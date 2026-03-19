use clap::Parser;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::io::{self, BufReader, Read as _};
use std::path::{Path, PathBuf};
use std::process;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const ELEVENLABS_BASE: &str = "https://api.elevenlabs.io";
const FILENAME_MAX_TEXT_LEN: usize = 48;
const HASH_SUFFIX_LEN: usize = 4;

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

#[derive(Parser)]
#[command(name = "announce", about = "ElevenLabs TTS CLI")]
struct Cli {
    /// Text to speak
    message: Option<String>,

    /// Named profile from config
    #[arg(long)]
    profile: Option<String>,

    /// Override voice (friendly name or raw ID)
    #[arg(long)]
    voice: Option<String>,

    /// Override speed
    #[arg(long)]
    speed: Option<f32>,

    /// English input → lookup pre-translated version
    #[arg(long, conflicts_with_all = ["language", "translate"])]
    lookup: Option<String>,

    /// Input is already in this language
    #[arg(long, conflicts_with_all = ["lookup", "translate"])]
    language: Option<String>,

    /// English input → fast model translates (future)
    #[arg(long, conflicts_with_all = ["lookup", "language"])]
    translate: Option<String>,

    /// Read plain text from stdin
    #[arg(long)]
    stdin: bool,

    /// Workspace path (for VOICE.lock resolution)
    #[arg(long)]
    project_dir: Option<PathBuf>,

    /// Skip cache lookup and storage
    #[arg(long)]
    no_cache: bool,

    /// Dry run (resolve config, check cache, but don't play)
    #[arg(long)]
    quiet: bool,

    /// Internal: play a PCM file and exit (used by self-exec for background playback)
    #[arg(long, hide = true)]
    _play_pcm: Option<PathBuf>,

    /// Internal: play an MP3/audio file and exit (used by self-exec for background playback)
    #[arg(long, hide = true)]
    _play_file: Option<PathBuf>,
}

// ---------------------------------------------------------------------------
// Config types
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct VoiceDef {
    id: String,
    speed: Option<f32>,
}

#[derive(Deserialize, Default)]
struct Config {
    #[serde(default)]
    voices: HashMap<String, VoiceDef>,
    #[serde(default)]
    default: ProfileSettings,
    #[serde(default)]
    profile: HashMap<String, ProfileSettings>,
}

#[derive(Deserialize, Default)]
struct ProfileSettings {
    voice: Option<String>,
    model: Option<String>,
    speed: Option<f32>,
    output_format: Option<String>,
    language_code: Option<String>,
}

#[derive(Deserialize, Default)]
struct LookupTable {
    #[serde(default)]
    messages: HashMap<String, String>,
}

#[derive(Deserialize, serde::Serialize, Default)]
struct HitsTable {
    #[serde(default)]
    hits: HashMap<String, u32>,
}

// ---------------------------------------------------------------------------
// Resolved settings (after merging all layers)
// ---------------------------------------------------------------------------

struct Resolved {
    voice_id: String,
    model: String,
    speed: f32,
    language_code: String,
    text: String,
    api_key: String,
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

fn main() {
    let args = Cli::parse();

    // --- Internal playback subcommands (spawned by self-exec) ---
    if let Some(ref pcm_path) = args._play_pcm {
        play_pcm_from_file(pcm_path);
        process::exit(0);
    }
    if let Some(ref file_path) = args._play_file {
        play_audio_file(file_path);
        process::exit(0);
    }

    // --- SILENT.lock: global kill switch ---
    let voice_dir = resolve_voice_dir();
    if Path::new(&voice_dir).join("SILENT.lock").exists() {
        process::exit(0);
    }

    // --- Get input text ---
    let text = get_input_text(&args);

    // --- Translate stub ---
    if args.translate.is_some() {
        eprintln!("announce: --translate is not yet implemented, falling back to English");
    }

    // --- Load config ---
    let config = load_config(&voice_dir);

    // --- Resolve settings (priority: CLI > VOICE.lock > profile > default) ---
    let resolved = resolve_settings(&args, &config, &voice_dir, text);

    if args.quiet {
        eprintln!(
            "announce [dry-run]: voice={} lang={} speed={} text={:?}",
            resolved.voice_id, resolved.language_code, resolved.speed, resolved.text
        );
        process::exit(0);
    }

    // --- Ensure voice symlinks ---
    ensure_voice_symlinks(&config.voices);

    // --- Lookup table ---
    let (final_text, final_lang) = apply_lookup(&args, &resolved);

    // --- Cache check ---
    let audio_dir = resolve_audio_dir();
    let cache_hash = compute_hash(&final_text, &resolved.voice_id, resolved.speed, &resolved.model, &final_lang);
    let hash_short = &cache_hash[..HASH_SUFFIX_LEN];
    let cache_path = build_cache_path(&audio_dir, &resolved.voice_id, &final_lang, &final_text, hash_short);

    let proj_dir = args.project_dir.as_deref();

    if !args.no_cache && cache_path.exists() {
        play_file_forked(&cache_path, proj_dir);
        process::exit(0);
    }

    // --- Hit count logic ---
    let hits_path = Path::new(&voice_dir).join("hits.toml");
    let mut hits = load_hits(&hits_path);
    let count = hits.hits.get(&cache_hash).copied().unwrap_or(0);

    if count == 0 && !args.no_cache {
        // Request #1: stream PCM, play immediately
        hits.hits.insert(cache_hash, 1);
        save_hits(&hits_path, &hits);
        stream_and_play(&resolved, &final_text, &final_lang, proj_dir);
    } else if count == 1 && !args.no_cache {
        // Request #2: download MP3, cache, play from file
        hits.hits.insert(cache_hash, 2);
        save_hits(&hits_path, &hits);
        download_cache_and_play(&resolved, &final_text, &final_lang, &cache_path, proj_dir);
    } else {
        // no-cache mode or unexpected: just stream
        stream_and_play(&resolved, &final_text, &final_lang, proj_dir);
    }
}

// ---------------------------------------------------------------------------
// Input
// ---------------------------------------------------------------------------

fn get_input_text(arguments: &Cli) -> String {
    if arguments.stdin {
        let mut buf = String::new();
        io::stdin().read_to_string(&mut buf).unwrap_or_default();
        let text = buf.trim().to_string();
        if text.is_empty() {
            eprintln!("announce: no input on stdin");
            process::exit(1);
        }
        text
    } else if let Some(ref msg) = arguments.message {
        if msg.trim().is_empty() {
            eprintln!("announce: empty message");
            process::exit(1);
        }
        msg.trim().to_string()
    } else {
        eprintln!("announce: no message provided (use positional arg or --stdin)");
        process::exit(1);
    }
}

// ---------------------------------------------------------------------------
// Config loading
// ---------------------------------------------------------------------------

fn resolve_voice_dir() -> String {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    format!("{home}/.ai/voice")
}

fn resolve_audio_dir() -> String {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    format!("{home}/.ai/audio")
}

fn load_config(voice_dir: &str) -> Config {
    let path = Path::new(voice_dir).join("announce.toml");
    match fs::read_to_string(&path) {
        Ok(contents) => toml::from_str(&contents).unwrap_or_else(|e| {
            eprintln!("announce: config parse error: {e}");
            Config::default()
        }),
        Err(_) => Config::default(),
    }
}

fn load_api_key(voice_dir: &str) -> String {
    let env_path = Path::new(voice_dir).join(".env");
    if env_path.exists() {
        let _ = dotenvy::from_path(&env_path);
    }
    std::env::var("ELEVENLABS_API_KEY").unwrap_or_else(|_| {
        eprintln!("announce: ELEVENLABS_API_KEY not set (check ~/.ai/voice/.env)");
        process::exit(1);
    })
}

fn load_hits(path: &Path) -> HitsTable {
    match fs::read_to_string(path) {
        Ok(contents) => toml::from_str(&contents).unwrap_or_default(),
        Err(_) => HitsTable::default(),
    }
}

fn save_hits(path: &Path, table: &HitsTable) {
    if let Ok(serialized) = toml::to_string(table) {
        let _ = fs::write(path, serialized);
    }
}

// ---------------------------------------------------------------------------
// Settings resolution
// ---------------------------------------------------------------------------

fn resolve_settings(arguments: &Cli, config: &Config, voice_dir: &str, text: String) -> Resolved {
    let defaults = &config.default;

    // Layer 1: profile
    let profile = arguments.profile.as_ref().and_then(|name| config.profile.get(name));

    // Layer 2: VOICE.lock
    let voice_lock = arguments.project_dir.as_ref().and_then(|dir| {
        let path = dir.join("VOICE.lock");
        fs::read_to_string(&path).ok().and_then(|s| toml::from_str::<ProfileSettings>(&s).ok())
    });

    // Merge: CLI > VOICE.lock > profile > default
    let voice_name = arguments.voice.as_deref()
        .or_else(|| voice_lock.as_ref().and_then(|v| v.voice.as_deref()))
        .or_else(|| profile.and_then(|p| p.voice.as_deref()))
        .or_else(|| defaults.voice.as_deref())
        .unwrap_or("alloy");

    let voice_def = config.voices.get(voice_name);
    let voice_id = voice_def
        .map(|v| v.id.as_str())
        .unwrap_or(voice_name)
        .to_string();

    let model = profile.and_then(|p| p.model.as_deref())
        .or(defaults.model.as_deref())
        .unwrap_or("eleven_multilingual_v2")
        .to_string();

    let speed = arguments.speed
        .or_else(|| voice_lock.as_ref().and_then(|v| v.speed))
        .or_else(|| profile.and_then(|p| p.speed))
        .or_else(|| voice_def.and_then(|v| v.speed))
        .or(defaults.speed)
        .unwrap_or(1.0);

    let language_code = arguments.language.as_deref()
        .or(arguments.lookup.as_deref())
        .or(arguments.translate.as_deref())
        .or_else(|| voice_lock.as_ref().and_then(|v| v.language_code.as_deref()))
        .or_else(|| profile.and_then(|p| p.language_code.as_deref()))
        .or(defaults.language_code.as_deref())
        .unwrap_or("en")
        .to_string();

    let speed = (speed.clamp(0.7, 1.2) * 100.0).round() / 100.0;

    let api_key = load_api_key(voice_dir);

    Resolved {
        voice_id,
        model,
        speed,
        language_code,
        text,
        api_key,
    }
}

// ---------------------------------------------------------------------------
// Lookup tables
// ---------------------------------------------------------------------------

/// Returns (text_to_speak, language_code). If lookup matches, returns the translated
/// text and the lookup language. Otherwise returns the original text and resolved language.
fn apply_lookup(arguments: &Cli, resolved: &Resolved) -> (String, String) {
    if let Some(ref lang_code) = arguments.lookup {
        let voice_dir = resolve_voice_dir();
        let lookup_path = Path::new(&voice_dir).join("lookups").join(format!("{lang_code}.toml"));
        if let Ok(contents) = fs::read_to_string(&lookup_path) {
            if let Ok(table) = toml::from_str::<LookupTable>(&contents) {
                if let Some(translated) = table.messages.get(&resolved.text) {
                    return (translated.to_string(), lang_code.to_string());
                }
            }
        }
    }
    (resolved.text.to_string(), resolved.language_code.to_string())
}

// ---------------------------------------------------------------------------
// Voice symlinks
// ---------------------------------------------------------------------------

fn ensure_voice_symlinks(voices: &HashMap<String, VoiceDef>) {
    let audio_dir_str = resolve_audio_dir();
    let audio_dir = Path::new(&audio_dir_str);
    let _ = fs::create_dir_all(audio_dir);

    for (name, def) in voices {
        let link_path = audio_dir.join(name);
        let target = audio_dir.join(&def.id);
        let _ = fs::create_dir_all(&target);

        match fs::read_link(&link_path) {
            Ok(existing) if existing == target => {}
            _ => {
                let _ = fs::remove_file(&link_path);
                let _ = std::os::unix::fs::symlink(&target, &link_path);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Cache
// ---------------------------------------------------------------------------

fn compute_hash(text: &str, voice_id: &str, speed: f32, model: &str, lang: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    hasher.update(voice_id.as_bytes());
    hasher.update(speed.to_le_bytes());
    hasher.update(model.as_bytes());
    hasher.update(lang.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn sanitize_filename(text: &str) -> String {
    let cleaned: String = text
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() || c == ' ' { c } else { ' ' })
        .collect();
    let words: Vec<&str> = cleaned.split_whitespace().collect();
    let joined = words.join("_");
    if joined.len() > FILENAME_MAX_TEXT_LEN {
        joined[..FILENAME_MAX_TEXT_LEN].trim_end_matches('_').to_string()
    } else {
        joined
    }
}

fn build_cache_path(audio_dir: &str, voice_id: &str, lang: &str, text: &str, hash_short: &str) -> PathBuf {
    let dir = Path::new(audio_dir).join(voice_id).join(lang);
    let _ = fs::create_dir_all(&dir);
    let name = sanitize_filename(text);
    dir.join(format!("{name}_{hash_short}.mp3"))
}

// ---------------------------------------------------------------------------
// ElevenLabs API
// ---------------------------------------------------------------------------

fn build_tts_body(text: &str, model: &str, speed: f32, lang: &str) -> serde_json::Value {
    // Round speed to 2 decimal places and widen to f64 for precise JSON serialization
    let speed_rounded = ((speed as f64) * 100.0).round() / 100.0;
    serde_json::json!({
        "text": text,
        "model_id": model,
        "voice_settings": {
            "speed": speed_rounded
        },
        "language_code": lang
    })
}

fn stream_and_play(settings: &Resolved, text: &str, lang: &str, project_dir: Option<&Path>) {
    let url = format!(
        "{ELEVENLABS_BASE}/v1/text-to-speech/{}/stream?output_format=pcm_24000",
        settings.voice_id
    );
    let body = build_tts_body(text, &settings.model, settings.speed, lang);

    let client = reqwest::blocking::Client::new();
    let response = client
        .post(&url)
        .header("xi-api-key", &settings.api_key)
        .header("Content-Type", "application/json")
        .header("Accept", "audio/pcm")
        .json(&body)
        .send();

    match response {
        Ok(resp) if resp.status().is_success() => {
            let bytes = resp.bytes().unwrap_or_default();
            play_pcm_forked(&bytes, project_dir);
        }
        Ok(resp) => {
            eprintln!("announce: ElevenLabs API error {}: {}", resp.status(), resp.text().unwrap_or_default());
            process::exit(1);
        }
        Err(error) => {
            eprintln!("announce: request failed: {error}");
            process::exit(1);
        }
    }
}

fn download_cache_and_play(settings: &Resolved, text: &str, lang: &str, cache_path: &Path, project_dir: Option<&Path>) {
    let url = format!(
        "{ELEVENLABS_BASE}/v1/text-to-speech/{}?output_format=mp3_22050_32",
        settings.voice_id
    );
    let body = build_tts_body(text, &settings.model, settings.speed, lang);

    let client = reqwest::blocking::Client::new();
    let response = client
        .post(&url)
        .header("xi-api-key", &settings.api_key)
        .header("Content-Type", "application/json")
        .json(&body)
        .send();

    match response {
        Ok(resp) if resp.status().is_success() => {
            let bytes = resp.bytes().unwrap_or_default();
            if let Some(parent) = cache_path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            let _ = fs::write(cache_path, &bytes);
            play_file_forked(cache_path, project_dir);
        }
        Ok(resp) => {
            eprintln!("announce: ElevenLabs API error {}: {}", resp.status(), resp.text().unwrap_or_default());
            process::exit(1);
        }
        Err(error) => {
            eprintln!("announce: request failed: {error}");
            process::exit(1);
        }
    }
}

// ---------------------------------------------------------------------------
// Audio playback (self-exec — spawns a detached child process)
// ---------------------------------------------------------------------------

/// Write PCM data to a temp file, spawn `announce --_play-pcm <path>`, return immediately.
fn play_pcm_forked(pcm_data: &[u8], project_dir: Option<&Path>) {
    let temp_dir = std::env::temp_dir();
    let temp_path = temp_dir.join(format!("announce_{}.pcm", std::process::id()));
    if fs::write(&temp_path, pcm_data).is_err() {
        eprintln!("announce: failed to write temp PCM file");
        return;
    }

    let exe = match std::env::current_exe() {
        Ok(path) => path,
        Err(error) => {
            eprintln!("announce: cannot resolve own executable: {error}");
            return;
        }
    };

    let mut cmd = std::process::Command::new(exe);
    cmd.arg("--play-pcm").arg(&temp_path);
    if let Some(dir) = project_dir {
        cmd.arg("--project-dir").arg(dir);
    }
    let _ = cmd
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

/// Spawn `announce --_play-file <path>` as a detached process, return immediately.
fn play_file_forked(path: &Path, project_dir: Option<&Path>) {
    let exe = match std::env::current_exe() {
        Ok(path) => path,
        Err(error) => {
            eprintln!("announce: cannot resolve own executable: {error}");
            return;
        }
    };

    let mut cmd = std::process::Command::new(exe);
    cmd.arg("--play-file").arg(path);
    if let Some(dir) = project_dir {
        cmd.arg("--project-dir").arg(dir);
    }
    let _ = cmd
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

/// Internal: play PCM S16LE 24kHz from a file, then delete the temp file.
fn play_pcm_from_file(path: &Path) {
    use rodio::{OutputStream, Sink};

    let data = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) => {
            eprintln!("announce: cannot read {}: {error}", path.display());
            process::exit(1);
        }
    };

    // Clean up temp file
    let _ = fs::remove_file(path);

    let (_stream, stream_handle) = match OutputStream::try_default() {
        Ok(handles) => handles,
        Err(error) => {
            eprintln!("announce: audio output error: {error}");
            process::exit(1);
        }
    };

    let sink = match Sink::try_new(&stream_handle) {
        Ok(sink) => sink,
        Err(error) => {
            eprintln!("announce: sink creation failed: {error}");
            process::exit(1);
        }
    };

    let source = rodio::buffer::SamplesBuffer::new(1, 24000, pcm_s16le_to_f32(&data));
    sink.append(source);
    sink.sleep_until_end();
}

/// Internal: play an MP3/audio file via rodio decoder.
fn play_audio_file(path: &Path) {
    use rodio::{Decoder, OutputStream, Sink};

    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) => {
            eprintln!("announce: cannot open {}: {error}", path.display());
            process::exit(1);
        }
    };

    let (_stream, stream_handle) = match OutputStream::try_default() {
        Ok(handles) => handles,
        Err(error) => {
            eprintln!("announce: audio output error: {error}");
            process::exit(1);
        }
    };

    let sink = match Sink::try_new(&stream_handle) {
        Ok(sink) => sink,
        Err(error) => {
            eprintln!("announce: sink creation failed: {error}");
            process::exit(1);
        }
    };

    let source = match Decoder::new(BufReader::new(file)) {
        Ok(source) => source,
        Err(error) => {
            eprintln!("announce: decode error for {}: {error}", path.display());
            process::exit(1);
        }
    };

    sink.append(source);
    sink.sleep_until_end();
}

fn pcm_s16le_to_f32(data: &[u8]) -> Vec<f32> {
    data.chunks_exact(2)
        .map(|chunk| {
            let sample = i16::from_le_bytes([chunk[0], chunk[1]]);
            sample as f32 / 32768.0
        })
        .collect()
}

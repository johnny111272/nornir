use announce_core::{
    apply_lookup, build_cache_path, build_kokoro_body, build_tts_body, compute_hash,
    pcm_s16le_to_f32, resolve_settings, Backend, Config, HitsTable, LookupTable, ProfileSettings,
    ResolveInput, Resolved, ELEVENLABS_BASE, HASH_SUFFIX_LEN, KOKORO_BASE,
};
use clap::Parser;
use std::collections::HashMap;
use std::fs;
use std::io::{self, BufReader, Read as _};
use std::path::{Path, PathBuf};
use std::process;

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

    /// Source scope (for VOICE.lock resolution and process identification)
    #[arg(long)]
    source: Option<PathBuf>,

    /// Alert severity level (metadata for filtering/logging)
    #[arg(long)]
    severity: Option<String>,

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
// Main
// ---------------------------------------------------------------------------

fn main() {
    let args = Cli::parse();
    if let Err(e) = run(args) {
        eprintln!("announce: {e}");
        process::exit(1);
    }
}

fn run(args: Cli) -> Result<(), String> {
    // --- Internal playback subcommands (spawned by self-exec) ---
    if let Some(ref pcm_path) = args._play_pcm {
        return play_pcm_from_file(pcm_path);
    }
    if let Some(ref file_path) = args._play_file {
        return play_audio_file(file_path);
    }

    let control_voice = write_engine::ai_home().join("control/voice");

    // Resolve workspace from --source if provided
    let workspace = args.source.as_ref().and_then(|s| {
        workspace_registry::resolve_workspace_from_path(&s.display().to_string()).ok().flatten()
    });

    // --- SILENT.lock: global first, then per-workspace ---
    if control_voice.join("SILENT.lock").exists() {
        return Ok(());
    }
    if let Some(ref ws) = workspace {
        if workspace_registry::workspace_control_dir(ws).join("SILENT.lock").exists() {
            return Ok(());
        }
    }

    let text = get_input_text(&args)?;

    // --- RECORDING.lock: queue non-critical messages during voice recording ---
    if control_voice.join("RECORDING.lock").exists() {
        let is_critical = args.severity.as_deref() == Some("critical");
        if !is_critical {
            // iTerm2 notification so stale locks don't silently eat messages
            eprint!("\x1b]9;announce: queued (RECORDING.lock)\x07");
            return queue_for_later(&args, &text);
        }
    }

    if args.translate.is_some() {
        eprintln!("announce: --translate is not yet implemented, falling back to English");
    }

    let resolved = resolve_from_args(&args, &control_voice, workspace.as_deref(), text)?;

    if args.quiet {
        eprintln!(
            "announce [dry-run]: voice={} lang={} speed={} text={:?}",
            resolved.voice_id, resolved.language_code, resolved.speed, resolved.text
        );
        return Ok(());
    }

    let audio_dir = write_engine::ai_home().join("audio");
    synthesize_and_play(&args, &resolved, &control_voice, &audio_dir)
}

/// Load config, VOICE.lock, API key, and merge into Resolved.
fn resolve_from_args(args: &Cli, control_voice: &Path, workspace: Option<&str>, text: String) -> Result<Resolved, String> {
    let config = load_config(control_voice);
    let voice_lock = workspace.and_then(|ws| {
        let path = workspace_registry::workspace_control_dir(ws).join("VOICE.lock");
        fs::read_to_string(&path)
            .ok()
            .and_then(|s| toml::from_str::<ProfileSettings>(&s).ok())
    });
    let api_key = if config.backend == Backend::Elevenlabs {
        let secrets_dir = write_engine::ai_home().join("voice");
        load_api_key(&secrets_dir)?
    } else {
        String::new()
    };

    let input = ResolveInput {
        profile_name: args.profile.as_deref(),
        voice_override: args.voice.as_deref(),
        speed_override: args.speed,
        language: args.language.as_deref(),
        lookup: args.lookup.as_deref(),
        translate: args.translate.as_deref(),
        voice_lock: voice_lock.as_ref(),
    };
    Ok(resolve_settings(&input, &config, text, api_key))
}

/// Lookup, cache check, hit counting, and TTS playback.
fn synthesize_and_play(
    args: &Cli,
    resolved: &Resolved,
    control_voice: &Path,
    audio_dir: &Path,
) -> Result<(), String> {
    let config = load_config(control_voice);
    ensure_voice_symlinks(&config.voices, audio_dir);

    let lookup_table = args.lookup.as_ref().map(|lang_code| {
        let lookup_path = control_voice.join("lookups").join(format!("{lang_code}.toml"));
        fs::read_to_string(&lookup_path)
            .ok()
            .and_then(|contents| toml::from_str::<LookupTable>(&contents).ok())
            .unwrap_or_default()
    });
    let (final_text, final_lang) = apply_lookup(
        args.lookup.as_deref(),
        lookup_table.as_ref(),
        &resolved.text,
        &resolved.language_code,
    );

    let cache_hash = compute_hash(
        &final_text, &resolved.voice_id, resolved.speed, &resolved.model, &final_lang,
    );
    let hash_short = &cache_hash[..HASH_SUFFIX_LEN];
    let cache_path = build_cache_path(audio_dir, &resolved.voice_id, &final_lang, &final_text, hash_short);
    if let Some(parent) = cache_path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    let proj_dir = args.source.as_deref();

    if !args.no_cache && cache_path.exists() {
        play_file_forked(&cache_path, proj_dir);
        return Ok(());
    }

    match config.backend {
        Backend::Kokoro => {
            // Kokoro: always download + cache (local, no cost)
            kokoro_download_and_play(resolved, &final_text, &cache_path, proj_dir)
        }
        Backend::Elevenlabs => {
            // ElevenLabs: stream first hit, cache on second (save credits)
            let hits_path = control_voice.join("hits.toml");
            let mut hits = load_hits(&hits_path);
            let count = hits.hits.get(&cache_hash).copied().unwrap_or(0);

            if count == 0 && !args.no_cache {
                hits.hits.insert(cache_hash, 1);
                save_hits(&hits_path, &hits);
                stream_and_play(resolved, &final_text, &final_lang, proj_dir)
            } else if count == 1 && !args.no_cache {
                hits.hits.insert(cache_hash, 2);
                save_hits(&hits_path, &hits);
                download_cache_and_play(resolved, &final_text, &final_lang, &cache_path, proj_dir)
            } else {
                stream_and_play(resolved, &final_text, &final_lang, proj_dir)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Input
// ---------------------------------------------------------------------------

/// Serialize announce invocation to the voice queue for later replay.
/// Reconstructs argv from the parsed Cli struct so relay_mic_to_voice can replay it.
fn queue_for_later(args: &Cli, text: &str) -> Result<(), String> {
    let queue_path = write_engine::ai_home().join("voice/queue.jsonl");
    if let Some(parent) = queue_path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("create queue dir: {e}"))?;
    }

    let mut argv: Vec<String> = Vec::new();
    if let Some(ref p) = args.profile {
        argv.extend(["--profile".into(), p.clone()]);
    }
    if let Some(ref v) = args.voice {
        argv.extend(["--voice".into(), v.clone()]);
    }
    if let Some(ref s) = args.severity {
        argv.extend(["--severity".into(), s.clone()]);
    }
    if let Some(speed) = args.speed {
        argv.extend(["--speed".into(), speed.to_string()]);
    }
    if let Some(ref src) = args.source {
        argv.extend(["--source".into(), src.display().to_string()]);
    }
    if let Some(ref lang) = args.language {
        argv.extend(["--language".into(), lang.clone()]);
    }
    if let Some(ref lookup) = args.lookup {
        argv.extend(["--lookup".into(), lookup.clone()]);
    }
    if args.no_cache {
        argv.push("--no-cache".into());
    }

    let mut entry = serde_json::Map::new();
    entry.insert("args".into(), serde_json::json!(argv));
    entry.insert("text".into(), serde_json::json!(text));

    let line = serde_json::to_string(&entry)
        .map_err(|e| format!("serialize queue entry: {e}"))?;

    write_engine::append_line_fsync(&queue_path, &line)
}

fn get_input_text(arguments: &Cli) -> Result<String, String> {
    if arguments.stdin {
        let mut buf = String::new();
        io::stdin()
            .read_to_string(&mut buf)
            .map_err(|e| format!("stdin read error: {e}"))?;
        let text = buf.trim().to_string();
        if text.is_empty() {
            return Err("no input on stdin".into());
        }
        Ok(text)
    } else if let Some(ref msg) = arguments.message {
        if msg.trim().is_empty() {
            return Err("empty message".into());
        }
        Ok(msg.trim().to_string())
    } else {
        Err("no message provided (use positional arg or --stdin)".into())
    }
}

// ---------------------------------------------------------------------------
// Config loading
// ---------------------------------------------------------------------------

fn load_config(voice_dir: &Path) -> Config {
    let path = voice_dir.join("announce.toml");
    match fs::read_to_string(&path) {
        Ok(contents) => toml::from_str(&contents).unwrap_or_else(|e| {
            eprintln!("announce: config parse error: {e}");
            Config::default()
        }),
        Err(_) => Config::default(),
    }
}

fn load_api_key(voice_dir: &Path) -> Result<String, String> {
    let env_path = voice_dir.join(".env");
    if env_path.exists() {
        let _ = dotenvy::from_path(&env_path);
    }
    std::env::var("ELEVENLABS_API_KEY")
        .map_err(|_| "ELEVENLABS_API_KEY not set (check ~/.ai/voice/.env)".into())
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
// Voice symlinks
// ---------------------------------------------------------------------------

fn ensure_voice_symlinks(voices: &HashMap<String, announce_core::VoiceDef>, audio_dir: &Path) {
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
// Kokoro API (local)
// ---------------------------------------------------------------------------

fn kokoro_download_and_play(
    settings: &Resolved,
    text: &str,
    cache_path: &Path,
    project_dir: Option<&Path>,
) -> Result<(), String> {
    let url = format!("{KOKORO_BASE}/v1/audio/speech");
    let body = build_kokoro_body(text, &settings.voice_id, settings.speed);

    let client = reqwest::blocking::Client::new();
    let response = client
        .post(&url)
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .map_err(|e| format!("kokoro request failed: {e}"))?;

    if !response.status().is_success() {
        return Err(format!(
            "kokoro error {}: {}",
            response.status(),
            response.text().unwrap_or_default()
        ));
    }

    let bytes = response.bytes().unwrap_or_default();
    if let Some(parent) = cache_path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::write(cache_path, &bytes);
    play_file_forked(cache_path, project_dir);
    Ok(())
}

// ---------------------------------------------------------------------------
// ElevenLabs API
// ---------------------------------------------------------------------------

fn stream_and_play(
    settings: &Resolved,
    text: &str,
    lang: &str,
    project_dir: Option<&Path>,
) -> Result<(), String> {
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
        .send()
        .map_err(|e| format!("request failed: {e}"))?;

    if !response.status().is_success() {
        return Err(format!(
            "ElevenLabs API error {}: {}",
            response.status(),
            response.text().unwrap_or_default()
        ));
    }

    let bytes = response.bytes().unwrap_or_default();
    play_pcm_forked(&bytes, project_dir);
    Ok(())
}

fn download_cache_and_play(
    settings: &Resolved,
    text: &str,
    lang: &str,
    cache_path: &Path,
    project_dir: Option<&Path>,
) -> Result<(), String> {
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
        .send()
        .map_err(|e| format!("request failed: {e}"))?;

    if !response.status().is_success() {
        return Err(format!(
            "ElevenLabs API error {}: {}",
            response.status(),
            response.text().unwrap_or_default()
        ));
    }

    let bytes = response.bytes().unwrap_or_default();
    if let Some(parent) = cache_path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::write(cache_path, &bytes);
    play_file_forked(cache_path, project_dir);
    Ok(())
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
        cmd.arg("--source").arg(dir);
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
        cmd.arg("--source").arg(dir);
    }
    let _ = cmd
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

/// Internal: play PCM S16LE 24kHz from a file, then delete the temp file.
fn play_pcm_from_file(path: &Path) -> Result<(), String> {
    use rodio::{OutputStream, Sink};

    let data = fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;

    // Clean up temp file
    let _ = fs::remove_file(path);

    let (_stream, stream_handle) =
        OutputStream::try_default().map_err(|e| format!("audio output error: {e}"))?;

    let sink =
        Sink::try_new(&stream_handle).map_err(|e| format!("sink creation failed: {e}"))?;

    let source = rodio::buffer::SamplesBuffer::new(1, 24000, pcm_s16le_to_f32(&data));
    sink.append(source);
    sink.sleep_until_end();
    Ok(())
}

/// Internal: play an MP3/audio file via rodio decoder.
fn play_audio_file(path: &Path) -> Result<(), String> {
    use rodio::{Decoder, OutputStream, Sink};

    let file =
        fs::File::open(path).map_err(|e| format!("cannot open {}: {e}", path.display()))?;

    let (_stream, stream_handle) =
        OutputStream::try_default().map_err(|e| format!("audio output error: {e}"))?;

    let sink =
        Sink::try_new(&stream_handle).map_err(|e| format!("sink creation failed: {e}"))?;

    let source = Decoder::new(BufReader::new(file))
        .map_err(|e| format!("decode error for {}: {e}", path.display()))?;

    sink.append(source);
    sink.sleep_until_end();
    Ok(())
}

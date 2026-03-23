//! Relay mic mute state to CC /voice keybinding.
//!
//! Listens for CoreAudio input-device mute changes (e.g. from a MuteMe button)
//! and simulates a keybinding press/release to trigger CC's push-to-talk voice
//! dictation.
//!
//! Usage:
//!     relay_mic_to_voice                     # default: meta+k
//!     relay_mic_to_voice --key meta+k        # explicit key combo
//!     relay_mic_to_voice --verbose            # print state transitions

use clap::Parser;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;

// =============================================================================
// CLI
// =============================================================================

#[derive(Parser, Debug)]
#[command(about = "Relay mic mute state to CC /voice keybinding")]
struct Args {
    /// Key combo to simulate (e.g. meta+k, ctrl+shift+f19)
    #[arg(long, default_value = "meta+k")]
    key: String,

    /// Print state transitions to stderr
    #[arg(long)]
    verbose: bool,
}

// =============================================================================
// Key combo parsing
// =============================================================================

struct KeyCombo {
    modifiers: Vec<enigo::Key>,
    key: enigo::Key,
}

fn parse_key_combo(input: &str) -> Result<KeyCombo, String> {
    let parts: Vec<&str> = input.split('+').collect();
    if parts.is_empty() {
        return Err("empty key combo".into());
    }

    let mut modifiers = Vec::new();
    for part in &parts[..parts.len() - 1] {
        let modifier = match part.to_lowercase().as_str() {
            "meta" | "cmd" | "command" | "super" => enigo::Key::Meta,
            "ctrl" | "control" => enigo::Key::Control,
            "shift" => enigo::Key::Shift,
            "alt" | "option" => enigo::Key::Alt,
            other => return Err(format!("unknown modifier: {other}")),
        };
        modifiers.push(modifier);
    }

    let key_str = parts.last().ok_or("empty key combo")?;
    let key = parse_key(key_str)?;

    Ok(KeyCombo { modifiers, key })
}

fn parse_key(input: &str) -> Result<enigo::Key, String> {
    if let Some(ch) = input.chars().next() {
        if input.len() == 1 {
            return Ok(enigo::Key::Unicode(ch));
        }
    }
    match input.to_lowercase().as_str() {
        "space" => Ok(enigo::Key::Space),
        "return" | "enter" => Ok(enigo::Key::Return),
        "escape" | "esc" => Ok(enigo::Key::Escape),
        "tab" => Ok(enigo::Key::Tab),
        "f1" => Ok(enigo::Key::F1),
        "f2" => Ok(enigo::Key::F2),
        "f3" => Ok(enigo::Key::F3),
        "f4" => Ok(enigo::Key::F4),
        "f5" => Ok(enigo::Key::F5),
        "f6" => Ok(enigo::Key::F6),
        "f7" => Ok(enigo::Key::F7),
        "f8" => Ok(enigo::Key::F8),
        "f9" => Ok(enigo::Key::F9),
        "f10" => Ok(enigo::Key::F10),
        "f11" => Ok(enigo::Key::F11),
        "f12" => Ok(enigo::Key::F12),
        "f13" => Ok(enigo::Key::F13),
        "f14" => Ok(enigo::Key::F14),
        "f15" => Ok(enigo::Key::F15),
        "f16" => Ok(enigo::Key::F16),
        "f17" => Ok(enigo::Key::F17),
        "f18" => Ok(enigo::Key::F18),
        "f19" => Ok(enigo::Key::F19),
        "f20" => Ok(enigo::Key::F20),
        other => Err(format!("unknown key: {other}")),
    }
}

// =============================================================================
// CoreAudio mic mute listener
// =============================================================================

mod audio {
    use coreaudio_sys::*;
    use std::os::raw::c_void;

    /// Get the default input (microphone) device ID.
    pub fn default_input_device() -> Result<AudioDeviceID, String> {
        let mut device_id: AudioDeviceID = 0;
        let mut size = std::mem::size_of::<AudioDeviceID>() as u32;
        let mut address = AudioObjectPropertyAddress {
            mSelector: kAudioHardwarePropertyDefaultInputDevice,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain,
        };
        let status = unsafe {
            AudioObjectGetPropertyData(
                kAudioObjectSystemObject,
                &mut address,
                0,
                std::ptr::null(),
                &mut size,
                &mut device_id as *mut _ as *mut c_void,
            )
        };
        if status != 0 {
            return Err(format!("AudioObjectGetPropertyData failed: {status}"));
        }
        if device_id == 0 || device_id == kAudioDeviceUnknown {
            return Err("no default input device found".into());
        }
        Ok(device_id)
    }

    /// Read the current mute state of a device's input scope.
    pub fn is_input_muted(device_id: AudioDeviceID) -> Result<bool, String> {
        let mut muted: u32 = 0;
        let mut size = std::mem::size_of::<u32>() as u32;
        let mut address = AudioObjectPropertyAddress {
            mSelector: kAudioDevicePropertyMute,
            mScope: kAudioDevicePropertyScopeInput,
            mElement: kAudioObjectPropertyElementMain,
        };
        let status = unsafe {
            AudioObjectGetPropertyData(
                device_id,
                &mut address,
                0,
                std::ptr::null(),
                &mut size,
                &mut muted as *mut _ as *mut c_void,
            )
        };
        if status != 0 {
            return Err(format!("failed to read mute state: {status}"));
        }
        Ok(muted != 0)
    }

    /// Context passed to the CoreAudio property listener callback.
    pub struct ListenerContext {
        pub device_id: AudioDeviceID,
        pub callback: Box<dyn Fn(bool) + Send>,
    }

    /// CoreAudio property listener callback — called on mute state changes.
    unsafe extern "C" fn mute_listener_proc(
        _object_id: AudioObjectID,
        _num_addresses: u32,
        _addresses: *const AudioObjectPropertyAddress,
        client_data: *mut c_void,
    ) -> OSStatus {
        let ctx = &*(client_data as *const ListenerContext);
        if let Ok(muted) = is_input_muted(ctx.device_id) {
            (ctx.callback)(muted);
        }
        0
    }

    /// Register a listener for mute state changes on the given device.
    /// Returns a boxed context that must be kept alive for the listener duration.
    pub fn register_mute_listener(
        device_id: AudioDeviceID,
        callback: impl Fn(bool) + Send + 'static,
    ) -> Result<Box<ListenerContext>, String> {
        let ctx = Box::new(ListenerContext {
            device_id,
            callback: Box::new(callback),
        });

        let mut address = AudioObjectPropertyAddress {
            mSelector: kAudioDevicePropertyMute,
            mScope: kAudioDevicePropertyScopeInput,
            mElement: kAudioObjectPropertyElementMain,
        };

        let status = unsafe {
            AudioObjectAddPropertyListener(
                device_id,
                &mut address,
                Some(mute_listener_proc),
                &*ctx as *const ListenerContext as *mut c_void,
            )
        };
        if status != 0 {
            return Err(format!("AudioObjectAddPropertyListener failed: {status}"));
        }
        Ok(ctx)
    }
}

// =============================================================================
// Keystroke simulation
// =============================================================================

fn press_combo(enigo: &mut enigo::Enigo, combo: &KeyCombo) {
    use enigo::{Direction::Press, Keyboard};
    for modifier in &combo.modifiers {
        let _ = enigo.key(*modifier, Press);
    }
    let _ = enigo.key(combo.key, Press);
}

fn release_combo(enigo: &mut enigo::Enigo, combo: &KeyCombo) {
    use enigo::{Direction::Release, Keyboard};
    let _ = enigo.key(combo.key, Release);
    for modifier in combo.modifiers.iter().rev() {
        let _ = enigo.key(*modifier, Release);
    }
}

// =============================================================================
// Signal handling
// =============================================================================

static SHUTDOWN: AtomicBool = AtomicBool::new(false);

extern "C" fn shutdown_handler(_sig: i32) {
    SHUTDOWN.store(true, Ordering::Relaxed);
}

fn install_signal_handlers() {
    unsafe {
        libc::signal(libc::SIGINT, shutdown_handler as libc::sighandler_t);
        libc::signal(libc::SIGTERM, shutdown_handler as libc::sighandler_t);
    }
}

// =============================================================================
// Run
// =============================================================================

fn run(args: Args) -> Result<(), String> {
    let combo = parse_key_combo(&args.key)?;
    let verbose = args.verbose;

    eprintln!("relay_mic_to_voice: listening for mic mute changes, key={}", args.key);

    let device_id = audio::default_input_device()?;
    let initial_muted = audio::is_input_muted(device_id)?;

    eprintln!(
        "relay_mic_to_voice: device={device_id}, initial_muted={initial_muted}"
    );

    // Channel: CoreAudio callback (any thread) → main thread (owns Enigo)
    let (tx, rx) = mpsc::channel::<bool>();

    let _listener_ctx = audio::register_mute_listener(device_id, move |muted| {
        let _ = tx.send(muted);
    })?;

    install_signal_handlers();

    let mut enigo = enigo::Enigo::new(&enigo::Settings::default())
        .map_err(|e| format!("enigo init failed: {e}"))?;

    // Main loop: drain channel, check shutdown
    while !SHUTDOWN.load(Ordering::Relaxed) {
        match rx.recv_timeout(std::time::Duration::from_millis(200)) {
            Ok(muted) => {
                if verbose {
                    eprintln!(
                        "relay_mic_to_voice: mic {}",
                        if muted { "muted → key up" } else { "unmuted → key down" }
                    );
                }
                if muted {
                    release_combo(&mut enigo, &combo);
                } else {
                    press_combo(&mut enigo, &combo);
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    // Release any held keys on shutdown
    release_combo(&mut enigo, &combo);

    eprintln!("relay_mic_to_voice: shutdown");
    Ok(())
}

// =============================================================================
// Entry point
// =============================================================================

fn main() {
    let args = Args::parse();
    if let Err(e) = run(args) {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_meta_k() {
        let combo = parse_key_combo("meta+k").unwrap();
        assert_eq!(combo.modifiers.len(), 1);
        assert_eq!(combo.key, enigo::Key::Unicode('k'));
    }

    #[test]
    fn parse_ctrl_shift_f19() {
        let combo = parse_key_combo("ctrl+shift+f19").unwrap();
        assert_eq!(combo.modifiers.len(), 2);
        assert_eq!(combo.key, enigo::Key::F19);
    }

    #[test]
    fn parse_single_key() {
        let combo = parse_key_combo("space").unwrap();
        assert!(combo.modifiers.is_empty());
        assert_eq!(combo.key, enigo::Key::Space);
    }

    #[test]
    fn parse_empty_fails() {
        assert!(parse_key_combo("").is_err());
    }

    #[test]
    fn parse_unknown_modifier_fails() {
        assert!(parse_key_combo("banana+k").is_err());
    }

    #[test]
    fn parse_unknown_key_fails() {
        assert!(parse_key_combo("meta+banana").is_err());
    }

    #[test]
    fn parse_cmd_alias() {
        let combo = parse_key_combo("cmd+j").unwrap();
        assert_eq!(combo.modifiers[0], enigo::Key::Meta);
    }

    #[test]
    fn parse_option_alias() {
        let combo = parse_key_combo("option+a").unwrap();
        assert_eq!(combo.modifiers[0], enigo::Key::Alt);
    }
}

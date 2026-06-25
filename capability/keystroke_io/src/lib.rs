//! keystroke_io — synthetic keyboard output with correct "hold" semantics.
//!
//! The hard part of driving another app via synthetic keystrokes is HOLDING a
//! combo. The naive approach — re-press the whole chord — makes apps that use
//! key-repeat for hold-detection read it as release+re-press and flip state
//! rapidly (Claude Code's voice mode cycles listening/processing). The OS,
//! holding a real key, repeats ONLY the key while the modifiers stay held.
//! `SyntheticHold` reproduces that exactly, behind a `start()`/`stop()` API —
//! the caller never touches press/repeat/release.
//!
//! ```ignore
//! use keystroke_io::{KeyCombo, SyntheticHold};
//! use std::time::Duration;
//!
//! let hold = SyntheticHold::new(KeyCombo::parse("ctrl+shift+k")?, Duration::from_millis(10))?;
//! hold.start();   // press chord, sustain (re-press key only, modifiers held)
//! hold.stop();    // release everything
//! ```

use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::Duration;

// =============================================================================
// Key combo + parsing
// =============================================================================

/// A parsed key combination: zero or more modifiers plus one main key.
pub struct KeyCombo {
    modifiers: Vec<enigo::Key>,
    key: enigo::Key,
}

impl KeyCombo {
    /// Parse a combo like `"ctrl+shift+k"`, `"meta+k"`, `"space"`, `"f19"`.
    pub fn parse(input: &str) -> Result<Self, String> {
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
// Low-level keystroke mechanics (private — the knowledge that must stay hidden)
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

/// Re-press only the main key, leaving modifiers held — mimics OS key-repeat,
/// which repeats the key but NOT the modifiers. Re-pressing the whole chord
/// makes key-repeat-based hold detection read it as release+re-press.
fn repress_key(enigo: &mut enigo::Enigo, combo: &KeyCombo) {
    use enigo::{Direction::Press, Keyboard};
    let _ = enigo.key(combo.key, Press);
}

// =============================================================================
// SyntheticHold — self-managing held keystroke
// =============================================================================

enum Cmd {
    Start,
    Stop,
    Tap,
    Shutdown,
}

/// A synthetic key combo you can hold and release. Owns a worker thread that
/// holds the `enigo` instance and runs the key-repeat sustain loop, so callers
/// only ever `start()` / `stop()` — the mechanics are entirely hidden.
pub struct SyntheticHold {
    tx: mpsc::Sender<Cmd>,
    worker: Option<JoinHandle<()>>,
}

impl SyntheticHold {
    /// Spawn the worker, which creates and owns the `enigo` instance on its own
    /// thread (the handle never crosses threads). `repeat` is the key-repeat
    /// interval used while holding. Returns `Err` if the synthetic-input backend
    /// fails to initialize.
    pub fn new(combo: KeyCombo, repeat: Duration) -> Result<Self, String> {
        let (tx, rx) = mpsc::channel::<Cmd>();
        let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();

        let worker = std::thread::spawn(move || {
            let mut enigo = match enigo::Enigo::new(&enigo::Settings::default()) {
                Ok(enigo) => {
                    let _ = ready_tx.send(Ok(()));
                    enigo
                }
                Err(error) => {
                    let _ = ready_tx.send(Err(format!("enigo init failed: {error}")));
                    return;
                }
            };
            worker_loop(&mut enigo, &combo, repeat, &rx);
            release_combo(&mut enigo, &combo);
        });

        match ready_rx.recv() {
            Ok(Ok(())) => Ok(Self { tx, worker: Some(worker) }),
            Ok(Err(error)) => {
                let _ = worker.join();
                Err(error)
            }
            Err(_) => Err("synthetic hold worker exited during init".into()),
        }
    }

    /// Press the combo and begin sustaining the hold.
    pub fn start(&self) {
        let _ = self.tx.send(Cmd::Start);
    }

    /// Release the combo and stop sustaining.
    pub fn stop(&self) {
        let _ = self.tx.send(Cmd::Stop);
    }

    /// One discrete press+release. For non-hold use; do not interleave with a hold.
    pub fn tap(&self) {
        let _ = self.tx.send(Cmd::Tap);
    }
}

impl Drop for SyntheticHold {
    fn drop(&mut self) {
        let _ = self.tx.send(Cmd::Shutdown);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn worker_loop(
    enigo: &mut enigo::Enigo,
    combo: &KeyCombo,
    repeat: Duration,
    rx: &mpsc::Receiver<Cmd>,
) {
    let idle_poll = Duration::from_millis(200);
    let mut holding = false;

    loop {
        let timeout = if holding { repeat } else { idle_poll };
        match rx.recv_timeout(timeout) {
            Ok(Cmd::Start) => {
                if !holding {
                    press_combo(enigo, combo);
                    holding = true;
                }
            }
            Ok(Cmd::Stop) => {
                if holding {
                    release_combo(enigo, combo);
                    holding = false;
                }
            }
            Ok(Cmd::Tap) => {
                press_combo(enigo, combo);
                release_combo(enigo, combo);
            }
            Ok(Cmd::Shutdown) => {
                if holding {
                    release_combo(enigo, combo);
                }
                break;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if holding {
                    repress_key(enigo, combo);
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                if holding {
                    release_combo(enigo, combo);
                }
                break;
            }
        }
    }
}

// =============================================================================
// Tests (parsing — pure, no Enigo instance needed)
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_meta_k() {
        let combo = KeyCombo::parse("meta+k").unwrap();
        assert_eq!(combo.modifiers.len(), 1);
        assert_eq!(combo.key, enigo::Key::Unicode('k'));
    }

    #[test]
    fn parse_ctrl_shift_f19() {
        let combo = KeyCombo::parse("ctrl+shift+f19").unwrap();
        assert_eq!(combo.modifiers.len(), 2);
        assert_eq!(combo.key, enigo::Key::F19);
    }

    #[test]
    fn parse_single_key() {
        let combo = KeyCombo::parse("space").unwrap();
        assert!(combo.modifiers.is_empty());
        assert_eq!(combo.key, enigo::Key::Space);
    }

    #[test]
    fn parse_empty_fails() {
        assert!(KeyCombo::parse("").is_err());
    }

    #[test]
    fn parse_unknown_modifier_fails() {
        assert!(KeyCombo::parse("banana+k").is_err());
    }

    #[test]
    fn parse_unknown_key_fails() {
        assert!(KeyCombo::parse("meta+banana").is_err());
    }

    #[test]
    fn parse_cmd_alias() {
        let combo = KeyCombo::parse("cmd+j").unwrap();
        assert_eq!(combo.modifiers[0], enigo::Key::Meta);
    }

    #[test]
    fn parse_option_alias() {
        let combo = KeyCombo::parse("option+a").unwrap();
        assert_eq!(combo.modifiers[0], enigo::Key::Alt);
    }
}

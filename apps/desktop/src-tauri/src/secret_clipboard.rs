//! Copying a secret (the password of an exported Word file) to the clipboard (STANDARDS §5.9).
//!
//! * Kept out of clipboard history and cloud sync: Windows "clipboard history" (Win+V) and
//!   "sync across devices", the nspasteboard convention on macOS, the KDE password hint on Linux.
//! * Cleared after 60 seconds, and at once when the vault locks, but only if the clipboard
//!   still holds that secret: whatever was copied since is left alone.
//!
//! Done here, not in the web view: the web view may not write to the clipboard while the window
//! is not in focus, which is exactly when the minute runs out (Einat is in Word).

use std::sync::{Arc, Mutex};
use std::time::Duration;

use arboard::Clipboard;
use zeroize::Zeroizing;

pub const CLEAR_AFTER: Duration = Duration::from_secs(60);

#[derive(Default)]
struct Held {
    /// On Linux the clipboard is served by this object; it must stay alive.
    clipboard: Option<Clipboard>,
    /// Wiped from memory when dropped.
    secret: Option<Zeroizing<String>>,
    generation: u64,
}

#[derive(Clone, Default)]
pub struct SecretClipboard {
    held: Arc<Mutex<Held>>,
}

impl SecretClipboard {
    /// Put `secret` on the clipboard, out of history and cloud sync, and clear it later.
    pub fn copy(&self, secret: &str) -> Result<(), String> {
        let generation = {
            let mut held = self.held.lock().map_err(|_| "clipboard lock".to_owned())?;
            if held.clipboard.is_none() {
                held.clipboard = Some(Clipboard::new().map_err(|e| e.to_string())?);
            }
            let clipboard = held
                .clipboard
                .as_mut()
                .ok_or_else(|| "no clipboard".to_owned())?;
            set_private(clipboard, secret)?;
            held.secret = Some(Zeroizing::new(secret.to_owned()));
            held.generation += 1;
            held.generation
        };
        let me = self.clone();
        std::thread::spawn(move || {
            std::thread::sleep(CLEAR_AFTER);
            me.clear_if(Some(generation));
        });
        Ok(())
    }

    /// The vault locked: take the secret off the clipboard now.
    pub fn clear_now(&self) {
        self.clear_if(None);
    }

    /// Clear if the clipboard still holds our secret (and, when given, it is still that copy).
    fn clear_if(&self, generation: Option<u64>) {
        let Ok(mut held) = self.held.lock() else {
            return;
        };
        if generation.is_some_and(|g| g != held.generation) {
            return;
        }
        let Some(secret) = held.secret.take() else {
            return;
        };
        if let Some(clipboard) = held.clipboard.as_mut() {
            if clipboard.get_text().is_ok_and(|t| t == secret.as_str()) {
                let _ = clipboard.clear();
            }
        }
    }
}

#[cfg(windows)]
fn set_private(clipboard: &mut Clipboard, secret: &str) -> Result<(), String> {
    use arboard::SetExtWindows;
    clipboard
        .set()
        .exclude_from_history()
        .exclude_from_cloud()
        .exclude_from_monitoring()
        .text(secret)
        .map_err(|e| e.to_string())
}

#[cfg(target_os = "macos")]
fn set_private(clipboard: &mut Clipboard, secret: &str) -> Result<(), String> {
    use arboard::SetExtApple;
    clipboard
        .set()
        .exclude_from_history()
        .text(secret)
        .map_err(|e| e.to_string())
}

#[cfg(target_os = "linux")]
fn set_private(clipboard: &mut Clipboard, secret: &str) -> Result<(), String> {
    use arboard::SetExtLinux;
    clipboard
        .set()
        .exclude_from_history()
        .text(secret)
        .map_err(|e| e.to_string())
}

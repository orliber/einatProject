//! Copying a secret (the password of an exported Word file) to the clipboard (STANDARDS §5.9).
//!
//! * Kept out of clipboard history and cloud sync: Windows "clipboard history" (Win+V) and
//!   "sync across devices"; on macOS the nspasteboard convention and "this Mac only", so it
//!   does not travel to her iPhone by Universal Clipboard (Handoff); the KDE password hint on
//!   Linux.
//! * Cleared after [`CLEAR_AFTER`] (60 seconds, 20 on a Mac), and at once when the vault locks,
//!   but only if the clipboard still holds that secret: whatever was copied since is left alone.
//!
//! Done here, not in the web view: the web view may not write to the clipboard while the window
//! is not in focus, which is exactly when the minute runs out (Einat is in Word).

use std::sync::{Arc, Mutex};
use std::time::Duration;

use arboard::Clipboard;
use zeroize::Zeroizing;

/// How long a copied secret stays on the clipboard. Shorter on a Mac (D-125): what is left
/// there (an app that ignores the conventions above, Apple's own handling of "this Mac only")
/// can only be narrowed in time (THREAT_MODEL R15).
#[cfg(target_os = "macos")]
pub const CLEAR_AFTER: Duration = Duration::from_secs(20);
#[cfg(not(target_os = "macos"))]
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

/// On a Mac the general pasteboard is written directly (D-125): arboard marks the copy as
/// concealed (out of clipboard managers' history) but cannot keep it on this Mac, and without
/// `NSPasteboardContentsCurrentHostOnly` Universal Clipboard hands it to her iPhone and iPad.
#[cfg(target_os = "macos")]
fn set_private(_clipboard: &mut Clipboard, secret: &str) -> Result<(), String> {
    use objc2_app_kit::{NSPasteboard, NSPasteboardContentsOptions};
    use objc2_foundation::NSString;
    let pasteboard = NSPasteboard::generalPasteboard();
    // Empties the pasteboard and keeps what comes next on this Mac only.
    pasteboard.prepareForNewContentsWithOptions(NSPasteboardContentsOptions::CurrentHostOnly);
    let text = NSString::from_str(secret);
    // NSPasteboardTypeString, and the nspasteboard.org marker for "concealed" (a password).
    let written = pasteboard
        .setString_forType(&text, &NSString::from_str("public.utf8-plain-text"))
        && pasteboard.setString_forType(
            &NSString::from_str(""),
            &NSString::from_str("org.nspasteboard.ConcealedType"),
        );
    if written {
        Ok(())
    } else {
        pasteboard.clearContents();
        Err("the pasteboard refused the text".to_owned())
    }
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

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    /// On the Mac runner's own pasteboard: the copy is readable as plain text, carries the
    /// "concealed" marker, and is taken off again (D-125).
    #[test]
    fn a_secret_on_the_mac_pasteboard_is_concealed_and_cleared() {
        use objc2_app_kit::NSPasteboard;
        use objc2_foundation::NSString;
        let secret = "סיסמה-בדויה-לבדיקה";
        let clipboard = SecretClipboard::default();
        assert!(clipboard.copy(secret).is_ok());
        let pasteboard = NSPasteboard::generalPasteboard();
        let text = pasteboard.stringForType(&NSString::from_str("public.utf8-plain-text"));
        assert_eq!(text.map(|t| t.to_string()).as_deref(), Some(secret));
        assert!(pasteboard
            .stringForType(&NSString::from_str("org.nspasteboard.ConcealedType"))
            .is_some());
        clipboard.clear_now();
        let text = pasteboard.stringForType(&NSString::from_str("public.utf8-plain-text"));
        assert_ne!(text.map(|t| t.to_string()).as_deref(), Some(secret));
    }
}

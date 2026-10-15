//! The operating-system surface: focused app, selection, clipboard, synthetic keys.
//!
//! Platform crates implement the primitive operations. The composite behaviours
//! (paste with clipboard restore, selection via copy fallback) live here once,
//! so every platform shares them and tests can drive them with a mock.

use std::thread::sleep;
use std::time::Duration;

/// Identifies the app that will receive text. On macOS `id` is the bundle ID
/// (`com.tinyspeck.slackmacgap`), on Windows the executable name (`slack.exe`),
/// on Linux the X11 WM_CLASS (`slack`).
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AppInfo {
    pub id: Option<String>,
    pub name: Option<String>,
    /// Address-bar URL when the frontmost app is a browser and it can be read.
    pub url: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Keystroke {
    Paste,
    Copy,
    Enter,
}

/// Text of the focused editable element, as read through the accessibility API.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldSnapshot {
    pub value: String,
    /// Byte range of the selection inside `value`, if the API exposes it.
    pub selection: Option<std::ops::Range<usize>>,
}

pub trait Desktop: Send {
    fn frontmost_app(&self) -> AppInfo;
    /// Selected text through the accessibility API. `None` when the API is
    /// unavailable or nothing is selected; callers fall back to copy.
    fn ax_selected_text(&self) -> Option<String>;
    fn focused_field(&self) -> Option<FieldSnapshot>;
    fn clipboard_text(&self) -> Option<String>;
    fn set_clipboard_text(&self, text: &str) -> anyhow::Result<()>;
    fn send(&self, key: Keystroke) -> anyhow::Result<()>;
    fn notify(&self, title: &str, body: &str);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PasteTiming {
    /// Wait after writing the clipboard before sending the paste chord.
    pub settle: Duration,
    /// Wait after pasting before restoring the previous clipboard.
    pub restore_after: Duration,
}

impl Default for PasteTiming {
    fn default() -> Self {
        Self { settle: Duration::from_millis(30), restore_after: Duration::from_millis(250) }
    }
}

/// Pastes `text` into the focused app and restores the user's clipboard.
/// The restore is skipped if something else replaced the clipboard meanwhile.
pub fn paste_text(desktop: &dyn Desktop, text: &str, timing: PasteTiming) -> anyhow::Result<()> {
    let previous = desktop.clipboard_text();
    desktop.set_clipboard_text(text)?;
    sleep(timing.settle);
    desktop.send(Keystroke::Paste)?;
    sleep(timing.restore_after);
    if let Some(previous) = previous {
        if desktop.clipboard_text().as_deref() == Some(text) {
            desktop.set_clipboard_text(&previous)?;
        }
    }
    Ok(())
}

/// Reads the current selection: accessibility API first, then a synthetic copy
/// with the clipboard restored afterwards.
pub fn read_selection(desktop: &dyn Desktop, timing: PasteTiming) -> Option<String> {
    if let Some(text) = desktop.ax_selected_text().filter(|t| !t.is_empty()) {
        return Some(text);
    }
    let previous = desktop.clipboard_text();
    let sentinel = "\u{2063}openflow-selection-probe\u{2063}";
    desktop.set_clipboard_text(sentinel).ok()?;
    desktop.send(Keystroke::Copy).ok()?;
    sleep(timing.restore_after);
    let copied = desktop.clipboard_text();
    if let Some(previous) = previous {
        let _ = desktop.set_clipboard_text(&previous);
    }
    copied.filter(|t| t != sentinel && !t.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::MockDesktop;

    const FAST: PasteTiming = PasteTiming { settle: Duration::ZERO, restore_after: Duration::ZERO };

    #[test]
    fn paste_restores_previous_clipboard() {
        let d = MockDesktop::default();
        d.set_clipboard_text("user data").unwrap();
        paste_text(&d, "hello world", FAST).unwrap();
        assert_eq!(d.pasted(), vec!["hello world".to_string()]);
        assert_eq!(d.clipboard_text().as_deref(), Some("user data"));
    }

    #[test]
    fn paste_with_empty_clipboard_leaves_text() {
        let d = MockDesktop::default();
        paste_text(&d, "hi", FAST).unwrap();
        assert_eq!(d.pasted(), vec!["hi".to_string()]);
    }

    #[test]
    fn selection_prefers_accessibility() {
        let d = MockDesktop::default();
        d.state().ax_selection = Some("from ax".into());
        d.state().copy_yields = Some("from copy".into());
        assert_eq!(read_selection(&d, FAST).as_deref(), Some("from ax"));
    }

    #[test]
    fn selection_falls_back_to_copy_and_restores_clipboard() {
        let d = MockDesktop::default();
        d.set_clipboard_text("keep me").unwrap();
        d.state().copy_yields = Some("selected words".into());
        assert_eq!(read_selection(&d, FAST).as_deref(), Some("selected words"));
        assert_eq!(d.clipboard_text().as_deref(), Some("keep me"));
    }

    #[test]
    fn selection_is_none_when_copy_changes_nothing() {
        let d = MockDesktop::default();
        assert_eq!(read_selection(&d, FAST), None);
    }
}

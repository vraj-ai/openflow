//! Shared by every OS listener: feeds key transitions through the chord
//! tracker, forwards trigger events, and decides whether to swallow the key.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;

use openflow_core::hotkey::{ChordTracker, HotkeyConfig, Key, KeyEvent};

/// Set by the runtime while a recording is live, read by listeners to decide
/// whether Escape belongs to OpenFlow or to the focused app.
pub type RecordingFlag = Arc<AtomicBool>;

pub struct KeyRouter {
    tracker: ChordTracker,
    tx: Sender<KeyEvent>,
    recording: RecordingFlag,
}

impl KeyRouter {
    pub fn new(cfg: &HotkeyConfig, tx: Sender<KeyEvent>, recording: RecordingFlag) -> Self {
        Self { tracker: ChordTracker::new(cfg), tx, recording }
    }

    /// Returns true when the OS event should be swallowed.
    pub fn on_key(&mut self, key: Key, down: bool) -> bool {
        let engaged_before = self.tracker.is_engaged();
        let event = if down { self.tracker.key_down(key) } else { self.tracker.key_up(key) };
        let swallow = match key {
            Key::Escape => down && self.recording.load(Ordering::SeqCst),
            // Space inside the hands-free chord must not type a space.
            Key::Space => engaged_before || self.tracker.is_engaged(),
            _ => false,
        };
        if let Some(event) = event {
            if event != KeyEvent::Escape || self.recording.load(Ordering::SeqCst) {
                let _ = self.tx.send(event);
            }
        }
        swallow
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openflow_core::hotkey::Trigger;
    use std::sync::mpsc::channel;

    fn rig() -> (KeyRouter, std::sync::mpsc::Receiver<KeyEvent>, RecordingFlag) {
        let cfg = HotkeyConfig {
            dictate: vec![Key::Fn],
            command: vec![Key::Fn, Key::Ctrl],
            hands_free: vec![Key::Fn, Key::Space],
            min_hold_ms: 500,
            double_tap_ms: 350,
        };
        let (tx, rx) = channel();
        let flag = RecordingFlag::default();
        (KeyRouter::new(&cfg, tx, flag.clone()), rx, flag)
    }

    #[test]
    fn space_in_hands_free_chord_is_swallowed_but_plain_space_is_not() {
        let (mut r, rx, _) = rig();
        assert!(!r.on_key(Key::Space, true));
        assert!(!r.on_key(Key::Space, false));
        assert!(!r.on_key(Key::Fn, true));
        assert!(r.on_key(Key::Space, true));
        assert!(r.on_key(Key::Space, false));
        r.on_key(Key::Fn, false);
        let events: Vec<_> = rx.try_iter().collect();
        assert_eq!(
            events,
            vec![KeyEvent::Pressed(Trigger::Dictate), KeyEvent::Pressed(Trigger::HandsFree), KeyEvent::Released]
        );
    }

    #[test]
    fn escape_only_belongs_to_openflow_while_recording() {
        let (mut r, rx, flag) = rig();
        assert!(!r.on_key(Key::Escape, true));
        assert_eq!(rx.try_iter().count(), 0);
        flag.store(true, Ordering::SeqCst);
        assert!(r.on_key(Key::Escape, true));
        assert_eq!(rx.try_iter().collect::<Vec<_>>(), vec![KeyEvent::Escape]);
    }
}

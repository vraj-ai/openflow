//! Push-to-talk, hands-free and Command Mode as one state machine.
//!
//! Platform listeners translate physical keys into [`KeyEvent`]s through a
//! [`ChordTracker`]; the [`Machine`] turns those into recording [`Action`]s.
//! Semantics follow Wispr Flow: hold to talk, a hold under `min_hold` is a tap
//! and is discarded, a double tap or the hands-free chord starts hands-free
//! recording that stops on the next press, and Escape cancels.

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Key {
    Fn,
    Ctrl,
    Alt,
    Shift,
    Meta,
    RightAlt,
    RightCtrl,
    Space,
    Escape,
    F(u8),
}

/// The three gestures a chord can trigger.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Trigger {
    Dictate,
    Command,
    HandsFree,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyEvent {
    Pressed(Trigger),
    /// Every configured chord is now released.
    Released,
    Escape,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HotkeyConfig {
    pub dictate: Vec<Key>,
    pub command: Vec<Key>,
    pub hands_free: Vec<Key>,
    pub min_hold_ms: u64,
    pub double_tap_ms: u64,
}

impl Default for HotkeyConfig {
    fn default() -> Self {
        if cfg!(target_os = "macos") {
            Self {
                dictate: vec![Key::Fn],
                command: vec![Key::Fn, Key::Ctrl],
                hands_free: vec![Key::Fn, Key::Space],
                min_hold_ms: 500,
                double_tap_ms: 350,
            }
        } else {
            Self {
                dictate: vec![Key::Ctrl, Key::Meta],
                command: vec![Key::Ctrl, Key::Meta, Key::Alt],
                hands_free: vec![Key::Ctrl, Key::Meta, Key::Space],
                min_hold_ms: 500,
                double_tap_ms: 350,
            }
        }
    }
}

/// Maps the set of physically held keys to trigger events. The most specific
/// fully-held chord wins, so holding Fn and then adding Ctrl upgrades a
/// dictation into a command. Releasing keys only reports `Released` once no
/// chord is held at all, so key-up order does not matter.
#[derive(Debug, Clone)]
pub struct ChordTracker {
    chords: Vec<(Trigger, BTreeSet<Key>)>,
    held: BTreeSet<Key>,
    active: Option<Trigger>,
}

impl ChordTracker {
    pub fn new(cfg: &HotkeyConfig) -> Self {
        let mut chords: Vec<(Trigger, BTreeSet<Key>)> = [
            (Trigger::Dictate, &cfg.dictate),
            (Trigger::Command, &cfg.command),
            (Trigger::HandsFree, &cfg.hands_free),
        ]
        .into_iter()
        .filter(|(_, keys)| !keys.is_empty())
        .map(|(t, keys)| (t, keys.iter().copied().collect()))
        .collect();
        chords.sort_by_key(|(_, keys)| std::cmp::Reverse(keys.len()));
        Self { chords, held: BTreeSet::new(), active: None }
    }

    fn best_match(&self) -> Option<Trigger> {
        self.chords.iter().find(|(_, keys)| keys.is_subset(&self.held)).map(|(t, _)| *t)
    }

    pub fn key_down(&mut self, key: Key) -> Option<KeyEvent> {
        if key == Key::Escape {
            return Some(KeyEvent::Escape);
        }
        if !self.held.insert(key) {
            return None; // auto-repeat
        }
        let best = self.best_match();
        if best.is_some() && best != self.active {
            self.active = best;
            return best.map(KeyEvent::Pressed);
        }
        None
    }

    pub fn key_up(&mut self, key: Key) -> Option<KeyEvent> {
        self.held.remove(&key);
        if self.active.is_some() && self.best_match().is_none() {
            self.active = None;
            return Some(KeyEvent::Released);
        }
        None
    }

    /// True while any key belonging to a configured chord is held; listeners use
    /// it to decide whether to swallow the event.
    pub fn is_engaged(&self) -> bool {
        self.active.is_some()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Dictation,
    Command,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    StartRecording(Mode),
    /// The recording continues but its result is now handled in this mode.
    SwitchMode(Mode),
    /// Stop and process the recording.
    Finish(Mode),
    /// Stop and discard the recording.
    Cancel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Idle,
    Holding { mode: Mode, since: Instant },
    /// Recording until the next press. `armed` turns true once the press that
    /// started hands-free has been released.
    HandsFree { mode: Mode, armed: bool },
    /// A gesture ended while keys are still down; swallow their release.
    Draining,
}

#[derive(Debug, Clone)]
pub struct Machine {
    state: State,
    last_tap: Option<Instant>,
    min_hold: Duration,
    double_tap: Duration,
}

impl Machine {
    pub fn new(cfg: &HotkeyConfig) -> Self {
        Self {
            state: State::Idle,
            last_tap: None,
            min_hold: Duration::from_millis(cfg.min_hold_ms),
            double_tap: Duration::from_millis(cfg.double_tap_ms),
        }
    }

    pub fn state(&self) -> State {
        self.state
    }

    pub fn is_recording(&self) -> bool {
        matches!(self.state, State::Holding { .. } | State::HandsFree { .. })
    }

    pub fn handle(&mut self, event: KeyEvent, now: Instant) -> Option<Action> {
        use KeyEvent::*;
        use State::*;
        let (next, action) = match (self.state, event) {
            (Holding { .. } | HandsFree { .. }, Escape) => (Draining, Some(Action::Cancel)),
            (_, Escape) => (self.state, None),

            (Idle, Pressed(trigger)) => {
                let double_tap = trigger == Trigger::Dictate
                    && self.last_tap.is_some_and(|t| now.duration_since(t) <= self.double_tap);
                self.last_tap = None;
                match trigger {
                    Trigger::HandsFree => (HandsFree { mode: Mode::Dictation, armed: false }, None),
                    _ if double_tap => (HandsFree { mode: Mode::Dictation, armed: false }, None),
                    Trigger::Dictate => (Holding { mode: Mode::Dictation, since: now }, None),
                    Trigger::Command => (Holding { mode: Mode::Command, since: now }, None),
                }
                .with_start()
            }
            (Idle | Draining, Released) => (Idle, None),
            (Draining, Pressed(_)) => (Draining, None),

            (Holding { mode, since }, Pressed(Trigger::Command)) if mode == Mode::Dictation => {
                (Holding { mode: Mode::Command, since }, Some(Action::SwitchMode(Mode::Command)))
            }
            (Holding { mode, .. }, Pressed(Trigger::HandsFree)) => (HandsFree { mode, armed: false }, None),
            (Holding { .. }, Pressed(_)) => (self.state, None),
            (Holding { mode, since }, Released) => {
                if now.duration_since(since) < self.min_hold {
                    self.last_tap = Some(now);
                    (Idle, Some(Action::Cancel))
                } else {
                    (Idle, Some(Action::Finish(mode)))
                }
            }

            (HandsFree { mode, armed: false }, Released) => (HandsFree { mode, armed: true }, None),
            (HandsFree { mode, armed: false }, Pressed(Trigger::Command)) if mode == Mode::Dictation => {
                (HandsFree { mode: Mode::Command, armed: false }, Some(Action::SwitchMode(Mode::Command)))
            }
            (HandsFree { armed: false, .. }, Pressed(_)) => (self.state, None),
            (HandsFree { mode, armed: true }, Pressed(_)) => (Draining, Some(Action::Finish(mode))),
            (HandsFree { armed: true, .. }, Released) => (self.state, None),
        };
        self.state = next;
        action
    }
}

trait WithStart {
    fn with_start(self) -> (State, Option<Action>);
}

impl WithStart for (State, Option<Action>) {
    fn with_start(self) -> (State, Option<Action>) {
        let mode = match self.0 {
            State::Holding { mode, .. } | State::HandsFree { mode, .. } => mode,
            _ => return self,
        };
        (self.0, Some(Action::StartRecording(mode)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> HotkeyConfig {
        HotkeyConfig {
            dictate: vec![Key::Fn],
            command: vec![Key::Fn, Key::Ctrl],
            hands_free: vec![Key::Fn, Key::Space],
            min_hold_ms: 500,
            double_tap_ms: 350,
        }
    }

    struct Rig {
        m: Machine,
        t0: Instant,
    }

    impl Rig {
        fn new() -> Self {
            Self { m: Machine::new(&cfg()), t0: Instant::now() }
        }
        fn at(&mut self, ms: u64, ev: KeyEvent) -> Option<Action> {
            self.m.handle(ev, self.t0 + Duration::from_millis(ms))
        }
    }

    use Action::*;
    use KeyEvent::*;

    #[test]
    fn hold_then_release_finishes_dictation() {
        let mut r = Rig::new();
        assert_eq!(r.at(0, Pressed(Trigger::Dictate)), Some(StartRecording(Mode::Dictation)));
        assert_eq!(r.at(1200, Released), Some(Finish(Mode::Dictation)));
        assert_eq!(r.m.state(), State::Idle);
    }

    #[test]
    fn tap_under_min_hold_is_discarded() {
        let mut r = Rig::new();
        r.at(0, Pressed(Trigger::Dictate));
        assert_eq!(r.at(200, Released), Some(Cancel));
    }

    #[test]
    fn double_tap_enters_hands_free_and_next_press_finishes() {
        let mut r = Rig::new();
        r.at(0, Pressed(Trigger::Dictate));
        r.at(100, Released);
        assert_eq!(r.at(300, Pressed(Trigger::Dictate)), Some(StartRecording(Mode::Dictation)));
        assert_eq!(r.at(400, Released), None);
        assert!(r.m.is_recording());
        assert_eq!(r.at(9000, Pressed(Trigger::Dictate)), Some(Finish(Mode::Dictation)));
        assert_eq!(r.at(9100, Released), None);
        assert_eq!(r.m.state(), State::Idle);
    }

    #[test]
    fn slow_second_tap_is_just_another_tap() {
        let mut r = Rig::new();
        r.at(0, Pressed(Trigger::Dictate));
        r.at(100, Released);
        r.at(1000, Pressed(Trigger::Dictate));
        assert_eq!(r.at(1100, Released), Some(Cancel));
    }

    #[test]
    fn hands_free_chord_starts_hands_free() {
        let mut r = Rig::new();
        assert_eq!(r.at(0, Pressed(Trigger::HandsFree)), Some(StartRecording(Mode::Dictation)));
        assert_eq!(r.at(50, Released), None);
        assert_eq!(r.at(5000, Pressed(Trigger::Dictate)), Some(Finish(Mode::Dictation)));
    }

    #[test]
    fn adding_space_while_holding_converts_to_hands_free() {
        let mut r = Rig::new();
        r.at(0, Pressed(Trigger::Dictate));
        assert_eq!(r.at(300, Pressed(Trigger::HandsFree)), None);
        assert_eq!(r.at(400, Released), None);
        assert!(r.m.is_recording());
    }

    #[test]
    fn adding_ctrl_upgrades_to_command_mode() {
        let mut r = Rig::new();
        r.at(0, Pressed(Trigger::Dictate));
        assert_eq!(r.at(100, Pressed(Trigger::Command)), Some(SwitchMode(Mode::Command)));
        assert_eq!(r.at(2000, Released), Some(Finish(Mode::Command)));
    }

    #[test]
    fn escape_cancels_and_swallows_release() {
        let mut r = Rig::new();
        r.at(0, Pressed(Trigger::Dictate));
        assert_eq!(r.at(800, Escape), Some(Cancel));
        assert_eq!(r.at(900, Released), None);
        assert_eq!(r.m.state(), State::Idle);
        assert_eq!(r.at(1000, Pressed(Trigger::Dictate)), Some(StartRecording(Mode::Dictation)));
    }

    #[test]
    fn escape_cancels_hands_free() {
        let mut r = Rig::new();
        r.at(0, Pressed(Trigger::HandsFree));
        r.at(50, Released);
        assert_eq!(r.at(3000, Escape), Some(Cancel));
        assert_eq!(r.m.state(), State::Draining);
        r.at(3100, Released);
        assert_eq!(r.m.state(), State::Idle);
    }

    #[test]
    fn escape_when_idle_does_nothing() {
        let mut r = Rig::new();
        assert_eq!(r.at(0, Escape), None);
    }

    #[test]
    fn tracker_picks_most_specific_chord_and_releases_once() {
        let mut t = ChordTracker::new(&cfg());
        assert_eq!(t.key_down(Key::Fn), Some(Pressed(Trigger::Dictate)));
        assert_eq!(t.key_down(Key::Fn), None);
        assert_eq!(t.key_down(Key::Ctrl), Some(Pressed(Trigger::Command)));
        assert_eq!(t.key_up(Key::Ctrl), None);
        assert_eq!(t.key_up(Key::Fn), Some(Released));
        assert!(!t.is_engaged());
    }

    #[test]
    fn tracker_reports_chord_when_modifier_pressed_first() {
        let mut t = ChordTracker::new(&cfg());
        assert_eq!(t.key_down(Key::Ctrl), None);
        assert_eq!(t.key_down(Key::Fn), Some(Pressed(Trigger::Command)));
        assert_eq!(t.key_up(Key::Fn), Some(Released));
        assert_eq!(t.key_up(Key::Ctrl), None);
    }

    #[test]
    fn tracker_escape_passes_through() {
        let mut t = ChordTracker::new(&cfg());
        assert_eq!(t.key_down(Key::Escape), Some(Escape));
    }
}

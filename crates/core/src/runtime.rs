//! The event loop that connects hotkeys, the microphone and the controller.
//! Platform code feeds `KeyEvent`s in; everything else is driven from here.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::controller::{Controller, Outcome, Report};
use crate::desktop::Desktop;
use crate::dictionary::DictEntry;
use crate::hotkey::{Action, KeyEvent, Machine, Mode};

/// A microphone that buffers between `start` and `stop`.
pub trait AudioSource {
    fn start(&mut self);
    /// Returns the recording as 16 kHz mono f32.
    fn stop(&mut self) -> Vec<f32>;
}

#[derive(Debug, Clone, PartialEq)]
pub enum Status {
    Recording(Mode),
    Processing(Mode),
    Cancelled,
    Done(Report),
    Failed(String),
}

/// Runs until the key-event channel closes. `on_learned` receives dictionary
/// entries learned from the user's corrections (see `learning.auto_learn`).
/// `sleep` is injected so tests can drive the edit watch without real waiting.
// One wiring point for the hotkeys, mic, controller and reporters; a parameter
// struct would only move the argument list somewhere else.
#[allow(clippy::too_many_arguments)]
pub fn run<D: Desktop>(
    keys: Receiver<KeyEvent>,
    machine: &mut Machine,
    audio: &mut dyn AudioSource,
    controller: &mut Controller<D>,
    recording: Arc<AtomicBool>,
    on_status: &mut dyn FnMut(Status),
    on_learned: &mut dyn FnMut(&[DictEntry]),
    sleep: &mut dyn FnMut(Duration),
) {
    // The post-paste edit watch blocks this loop, so key presses that land during
    // it are stashed here and replayed: otherwise a quick second dictation would
    // be handled late, as a press and a release at the same instant, and dropped
    // as a tap.
    let mut pending: VecDeque<KeyEvent> = VecDeque::new();
    loop {
        let event = match pending.pop_front() {
            Some(event) => event,
            None => match keys.recv() {
                Ok(event) => event,
                Err(_) => break, // every sender went away
            },
        };
        let Some(action) = machine.handle(event, Instant::now()) else { continue };
        match action {
            Action::StartRecording(mode) => {
                audio.start();
                on_status(Status::Recording(mode));
            }
            Action::SwitchMode(mode) => on_status(Status::Recording(mode)),
            Action::Cancel => {
                audio.stop();
                on_status(Status::Cancelled);
            }
            Action::Finish(mode) => {
                let samples = audio.stop();
                on_status(Status::Processing(mode));
                match controller.process(mode, &samples) {
                    Ok(report) => {
                        let inserted = matches!(report.outcome, Outcome::Inserted(_));
                        on_status(Status::Done(report));
                        if mode == Mode::Dictation && inserted {
                            recording.store(false, Ordering::SeqCst);
                            let interrupted = AtomicBool::new(false);
                            let learned = controller.learn_after_insert(&interrupted, &mut |wait| {
                                sleep(wait);
                                // A key press during the wait ends the watch and
                                // is handled by the next loop iteration.
                                while let Ok(event) = keys.try_recv() {
                                    pending.push_back(event);
                                    interrupted.store(true, Ordering::SeqCst);
                                }
                            });
                            if !learned.is_empty() {
                                on_learned(&learned);
                            }
                        }
                    }
                    Err(e) => on_status(Status::Failed(format!("{e:#}"))),
                }
            }
        }
        recording.store(machine.is_recording(), Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controller::{Engines, Outcome};
    use crate::desktop::PasteTiming;
    use crate::hotkey::{HotkeyConfig, Trigger};
    use crate::mock::{MockDesktop, ScriptedStt};
    use crate::settings::Settings;
    use std::sync::mpsc::channel;
    use std::time::Duration;

    struct FakeMic {
        live: bool,
        starts: usize,
    }

    impl AudioSource for FakeMic {
        fn start(&mut self) {
            self.live = true;
            self.starts += 1;
        }
        fn stop(&mut self) -> Vec<f32> {
            self.live = false;
            vec![0.1; 16000]
        }
    }

    #[test]
    fn the_event_loop_learns_from_a_correction_after_a_paste() {
        let desktop = MockDesktop::default();
        let engines = Engines {
            stt: Box::new(ScriptedStt::new(["I deployed it to cooper netties"])),
            multilingual: None,
            vad: None,
            llm: None,
        };
        let mut controller = Controller::new(Settings::default(), engines, desktop.clone(), None);
        controller.paste_timing = PasteTiming { settle: Duration::ZERO, restore_after: Duration::ZERO };
        // The field holds what was just pasted; the correction arrives while the
        // watch is sleeping. The injected sleep makes that deterministic.
        let pasted = "I deployed it to cooper netties.";
        desktop.state().field =
            Some(crate::desktop::FieldSnapshot { value: pasted.into(), selection: None });
        let mut polls = 0;
        let mut sleep = |_: Duration| {
            if polls == 0 {
                desktop.state().field = Some(crate::desktop::FieldSnapshot {
                    value: "I deployed it to Kubernetes".into(),
                    selection: None,
                });
            }
            polls += 1;
        };
        let cfg = HotkeyConfig { min_hold_ms: 5, ..HotkeyConfig::default() };
        let mut machine = Machine::new(&cfg);
        let (tx, rx) = channel();
        let feeder = std::thread::spawn(move || {
            tx.send(KeyEvent::Pressed(Trigger::Dictate)).unwrap();
            std::thread::sleep(Duration::from_millis(30));
            tx.send(KeyEvent::Released).unwrap();
        });
        let mut mic = FakeMic { live: false, starts: 0 };
        let mut learned: Vec<DictEntry> = vec![];
        run(
            rx,
            &mut machine,
            &mut mic,
            &mut controller,
            Arc::default(),
            &mut |_| {},
            &mut |entries| learned.extend_from_slice(entries),
            &mut sleep,
        );
        feeder.join().unwrap();
        assert!(polls > 1, "the watch stopped before the correction appeared");
        let terms: Vec<&str> = learned.iter().map(|e| e.term.as_str()).collect();
        assert_eq!(terms, vec!["kubernetes"], "the loop must learn from the field");
        assert_eq!(controller.settings.dictionary.hotwords(), vec!["kubernetes".to_string()]);
    }

    #[test]
    fn reports_learned_terms_from_the_edit_watch() {
        let desktop = MockDesktop::default();
        let engines = Engines {
            stt: Box::new(ScriptedStt::new(["I deployed it to cooper netties"])),
            multilingual: None,
            vad: None,
            llm: None,
        };
        let mut controller = Controller::new(Settings::default(), engines, desktop.clone(), None);
        controller.paste_timing = PasteTiming { settle: Duration::ZERO, restore_after: Duration::ZERO };
        controller.settings.learning.watch_ms = 1;
        // The field holds the user's correction before the watch starts; a
        // one-millisecond budget keeps the loop to a single non-blocking pass.
        desktop.state().field =
            Some(crate::desktop::FieldSnapshot { value: "I deployed it to Kubernetes".into(), selection: None });
        let mut learned: Vec<DictEntry> = vec![];
        assert!(controller.learn_after_insert(&AtomicBool::new(false), &mut |_| {}).is_empty());
        controller.process(Mode::Dictation, &[0.0; 16000]).unwrap();
        let got = controller.learn_after_insert(&AtomicBool::new(false), &mut |_| {});
        assert_eq!(got.len(), 1, "{got:?}");
        assert_eq!(got[0].term, "kubernetes");
        learned.extend_from_slice(&got);
        assert_eq!(controller.settings.dictionary.hotwords(), vec!["kubernetes".to_string()]);
    }

    #[test]
    fn a_press_during_the_edit_watch_is_replayed_not_dropped() {
        let desktop = MockDesktop::default();
        let engines = Engines {
            stt: Box::new(ScriptedStt::new(["Hello there", "Hello again"])),
            multilingual: None,
            vad: None,
            llm: None,
        };
        let mut controller = Controller::new(Settings::default(), engines, desktop.clone(), None);
        controller.paste_timing = PasteTiming { settle: Duration::ZERO, restore_after: Duration::ZERO };
        // The field never settles (each poll looks different), so the watch can
        // only end on a key press; without that it is allowed a minute.
        controller.settings.learning.watch_ms = 60_000;
        desktop.state().field =
            Some(crate::desktop::FieldSnapshot { value: "Hello there".into(), selection: None });
        let mut polls = 0u32;
        let mut sleep = |_: Duration| {
            polls += 1;
            desktop.state().field = Some(crate::desktop::FieldSnapshot {
                value: format!("a draft that keeps changing {polls}"),
                selection: None,
            });
        };
        let cfg = HotkeyConfig { min_hold_ms: 5, ..HotkeyConfig::default() };
        let mut machine = Machine::new(&cfg);
        let (tx, rx) = channel();
        let feeder = std::thread::spawn(move || {
            for _ in 0..2 {
                tx.send(KeyEvent::Pressed(Trigger::Dictate)).unwrap();
                std::thread::sleep(Duration::from_millis(30));
                tx.send(KeyEvent::Released).unwrap();
            }
            // Any key press ends the second dictation's watch too.
            tx.send(KeyEvent::Escape).unwrap();
        });
        let mut mic = FakeMic { live: false, starts: 0 };
        let mut statuses = vec![];
        let started = Instant::now();
        run(
            rx,
            &mut machine,
            &mut mic,
            &mut controller,
            Arc::default(),
            &mut |s| statuses.push(s),
            &mut |_| {},
            &mut sleep,
        );
        feeder.join().unwrap();
        assert!(started.elapsed() < Duration::from_secs(5), "watch blocked the loop for {:?}", started.elapsed());
        assert_eq!(mic.starts, 2, "the second press must start a recording, not vanish");
        assert_eq!(desktop.pasted(), vec!["Hello there".to_string(), "Hello again".to_string()]);
    }

    #[test]
    fn hold_release_pastes_and_tap_does_not() {
        let desktop = MockDesktop::default();
        let engines = Engines {
            stt: Box::new(ScriptedStt::new(["Hello from the loop."])),
            multilingual: None,
            vad: None,
            llm: None,
        };
        let mut controller = Controller::new(Settings::default(), engines, desktop.clone(), None);
        controller.paste_timing = PasteTiming { settle: Duration::ZERO, restore_after: Duration::ZERO };
        let cfg = HotkeyConfig { min_hold_ms: 50, ..HotkeyConfig::default() };
        let mut machine = Machine::new(&cfg);
        let (tx, rx) = channel();
        let feeder = std::thread::spawn(move || {
            tx.send(KeyEvent::Pressed(Trigger::Dictate)).unwrap();
            tx.send(KeyEvent::Released).unwrap();
            std::thread::sleep(Duration::from_millis(400));
            tx.send(KeyEvent::Pressed(Trigger::Dictate)).unwrap();
            std::thread::sleep(Duration::from_millis(80));
            tx.send(KeyEvent::Released).unwrap();
        });
        let mut mic = FakeMic { live: false, starts: 0 };
        let mut statuses = vec![];
        run(
            rx,
            &mut machine,
            &mut mic,
            &mut controller,
            Arc::default(),
            &mut |s| statuses.push(s),
            &mut |_| {},
            &mut |_| {},
        );
        feeder.join().unwrap();
        assert_eq!(mic.starts, 2);
        assert!(!mic.live);
        assert_eq!(desktop.pasted(), vec!["Hello from the loop".to_string()]);
        assert_eq!(statuses[0], Status::Recording(Mode::Dictation));
        assert_eq!(statuses[1], Status::Cancelled);
        assert!(matches!(&statuses[4], Status::Done(r) if r.outcome == Outcome::Inserted("Hello from the loop".into())));
    }
}

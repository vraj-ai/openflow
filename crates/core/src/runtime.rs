//! The event loop that connects hotkeys, the microphone and the controller.
//! Platform code feeds `KeyEvent`s in; everything else is driven from here.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::time::Instant;

use crate::controller::{Controller, Report};
use crate::desktop::Desktop;
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

/// Runs until the key-event channel closes.
pub fn run<D: Desktop>(
    keys: Receiver<KeyEvent>,
    machine: &mut Machine,
    audio: &mut dyn AudioSource,
    controller: &mut Controller<D>,
    recording: Arc<AtomicBool>,
    on_status: &mut dyn FnMut(Status),
) {
    for event in keys {
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
                    Ok(report) => on_status(Status::Done(report)),
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
        run(rx, &mut machine, &mut mic, &mut controller, Arc::default(), &mut |s| statuses.push(s));
        feeder.join().unwrap();
        assert_eq!(mic.starts, 2);
        assert!(!mic.live);
        assert_eq!(desktop.pasted(), vec!["Hello from the loop".to_string()]);
        assert_eq!(statuses[0], Status::Recording(Mode::Dictation));
        assert_eq!(statuses[1], Status::Cancelled);
        assert!(matches!(&statuses[4], Status::Done(r) if r.outcome == Outcome::Inserted("Hello from the loop".into())));
    }
}

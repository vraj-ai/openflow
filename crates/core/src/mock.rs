//! Test doubles for the engine and desktop traits. Public so the CLI and the
//! platform crate can run the full pipeline headless.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard};

use crate::desktop::{AppInfo, Desktop, FieldSnapshot, Keystroke};
use crate::engine::{ChatPrompt, GenOptions, SpeechToText, SttHints, TextGenerator};

#[derive(Debug, Default)]
pub struct DesktopState {
    pub app: AppInfo,
    pub clipboard: Option<String>,
    pub ax_selection: Option<String>,
    pub field: Option<FieldSnapshot>,
    /// What a synthetic Copy puts on the clipboard (the "selected" text).
    pub copy_yields: Option<String>,
    pub pasted: Vec<String>,
    pub keys: Vec<Keystroke>,
    pub notifications: Vec<(String, String)>,
}

/// Records every interaction. Clones share state, so a test can keep a handle
/// while the controller owns another.
#[derive(Debug, Default, Clone)]
pub struct MockDesktop(Arc<Mutex<DesktopState>>);

impl MockDesktop {
    pub fn state(&self) -> MutexGuard<'_, DesktopState> {
        self.0.lock().unwrap()
    }
    pub fn pasted(&self) -> Vec<String> {
        self.state().pasted.clone()
    }
}

impl Desktop for MockDesktop {
    fn frontmost_app(&self) -> AppInfo {
        self.state().app.clone()
    }
    fn ax_selected_text(&self) -> Option<String> {
        self.state().ax_selection.clone()
    }
    fn focused_field(&self) -> Option<FieldSnapshot> {
        self.state().field.clone()
    }
    fn clipboard_text(&self) -> Option<String> {
        self.state().clipboard.clone()
    }
    fn set_clipboard_text(&self, text: &str) -> anyhow::Result<()> {
        self.state().clipboard = Some(text.to_string());
        Ok(())
    }
    fn send(&self, key: Keystroke) -> anyhow::Result<()> {
        let mut s = self.state();
        s.keys.push(key);
        match key {
            Keystroke::Paste => {
                let text = s.clipboard.clone().unwrap_or_default();
                s.pasted.push(text);
            }
            Keystroke::Copy => {
                if let Some(sel) = s.copy_yields.clone() {
                    s.clipboard = Some(sel);
                }
            }
            Keystroke::Enter => {}
        }
        Ok(())
    }
    fn notify(&self, title: &str, body: &str) {
        self.state().notifications.push((title.into(), body.into()));
    }
}

/// Returns queued transcripts in order, recording the hints it was given.
#[derive(Debug, Default, Clone)]
pub struct ScriptedStt {
    pub outputs: Arc<Mutex<VecDeque<String>>>,
    pub seen_hints: Arc<Mutex<Vec<SttHints>>>,
}

impl ScriptedStt {
    pub fn new<I: IntoIterator<Item = S>, S: Into<String>>(outputs: I) -> Self {
        Self {
            outputs: Arc::new(Mutex::new(outputs.into_iter().map(Into::into).collect())),
            seen_hints: Default::default(),
        }
    }
}

impl SpeechToText for ScriptedStt {
    fn transcribe(&mut self, _samples: &[f32], hints: &SttHints) -> anyhow::Result<String> {
        self.seen_hints.lock().unwrap().push(hints.clone());
        Ok(self.outputs.lock().unwrap().pop_front().unwrap_or_default())
    }
}

/// Produces text from a closure and records every prompt it saw.
pub struct FnGenerator<F> {
    pub f: F,
    pub prompts: Arc<Mutex<Vec<ChatPrompt>>>,
}

impl<F: FnMut(&ChatPrompt) -> String + Send> FnGenerator<F> {
    pub fn new(f: F) -> Self {
        Self { f, prompts: Default::default() }
    }
}

impl<F: FnMut(&ChatPrompt) -> String + Send> TextGenerator for FnGenerator<F> {
    fn generate(&mut self, prompt: &ChatPrompt, _opts: &GenOptions) -> anyhow::Result<String> {
        self.prompts.lock().unwrap().push(prompt.clone());
        Ok((self.f)(prompt))
    }
}

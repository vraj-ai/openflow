//! Placeholder until the Whisper engine lands.
use std::path::Path;

use openflow_core::engine::{SpeechToText, SttHints};

pub struct Whisper;

impl Whisper {
    pub fn load(_dir: &Path) -> anyhow::Result<Self> {
        anyhow::bail!("Whisper is not implemented yet")
    }
}

impl SpeechToText for Whisper {
    fn transcribe(&mut self, _samples: &[f32], _hints: &SttHints) -> anyhow::Result<String> {
        unreachable!()
    }
}

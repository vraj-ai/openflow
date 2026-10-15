//! Traits the inference engines implement. The core never names a concrete model.

use std::ops::Range;

/// Every engine consumes mono f32 PCM at this rate.
pub const SAMPLE_RATE: u32 = 16_000;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SttHints {
    /// BCP-47 primary language subtag ("en", "hi"), or `None` for auto-detect.
    pub language: Option<String>,
    /// Dictionary terms. Whisper turns these into an initial prompt; Parakeet ignores them
    /// and relies on the dictionary's post-replace pass instead.
    pub hotwords: Vec<String>,
}

pub trait SpeechToText: Send {
    fn transcribe(&mut self, samples: &[f32], hints: &SttHints) -> anyhow::Result<String>;
}

pub trait VoiceActivity: Send {
    /// Sample ranges that contain speech, in ascending order.
    fn speech_spans(&mut self, samples: &[f32]) -> anyhow::Result<Vec<Range<usize>>>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatPrompt {
    pub system: String,
    pub user: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GenOptions {
    pub max_tokens: usize,
    /// 0.0 means greedy decoding.
    pub temperature: f64,
}

pub trait TextGenerator: Send {
    fn generate(&mut self, prompt: &ChatPrompt, opts: &GenOptions) -> anyhow::Result<String>;
}

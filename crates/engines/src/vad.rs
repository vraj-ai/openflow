//! Silero VAD v5 (ONNX). 512-sample windows preceded by 64 samples of context, 16 kHz.

use std::ops::Range;
use std::path::Path;

use anyhow::{Context, Result};
use ndarray::{Array0, Array2, ArrayD, ArrayViewD};
use openflow_core::engine::{SAMPLE_RATE, VoiceActivity};

use crate::ort_env;

const CHUNK: usize = 512;
const CONTEXT: usize = 64;
const PROB_START: f32 = 0.5;
const PROB_END: f32 = 0.35;
const END_SILENCE: usize = SAMPLE_RATE as usize / 10; // 100 ms below PROB_END closes a span
const MIN_SPEECH: usize = SAMPLE_RATE as usize / 4; // shorter spans are noise
const PAD: usize = SAMPLE_RATE as usize / 5; // 200 ms of slack on each side

pub struct SileroVad {
    session: ort::session::Session,
    state: ArrayD<f32>,
}

impl SileroVad {
    pub fn load(model_path: &Path) -> Result<Self> {
        let session = ort_env::session(model_path)?;
        Ok(Self { session, state: ndarray::Array3::<f32>::zeros((2, 1, 128)).into_dyn() })
    }

    fn speech_prob(&mut self, window: &[f32]) -> Result<f32> {
        let input = Array2::from_shape_vec((1, window.len()), window.to_vec())?;
        let sr = Array0::from_elem((), SAMPLE_RATE as i64);
        let outputs = self.session.run(ort::inputs![
            "input" => ort::value::TensorRef::from_array_view(input.view())?,
            "state" => ort::value::TensorRef::from_array_view(self.state.view())?,
            "sr" => ort::value::TensorRef::from_array_view(sr.view())?,
        ])?;

        let prob = outputs
            .get("output")
            .context("Silero VAD output `output` missing")?
            .try_extract_array::<f32>()?
            .iter()
            .next()
            .copied()
            .unwrap_or(0.0);
        let next_state: ArrayViewD<f32> = outputs
            .get("stateN")
            .context("Silero VAD output `stateN` missing")?
            .try_extract_array::<f32>()?;
        self.state.assign(&next_state);
        Ok(prob)
    }
}

impl VoiceActivity for SileroVad {
    fn speech_spans(&mut self, samples: &[f32]) -> Result<Vec<Range<usize>>> {
        self.state.fill(0.0);
        let mut raw: Vec<Range<usize>> = Vec::new();
        let mut context = [0f32; CONTEXT];
        let mut open_start: Option<usize> = None;
        let mut quiet_from: Option<usize> = None;

        for start in (0..samples.len()).step_by(CHUNK) {
            let end = (start + CHUNK).min(samples.len());
            let mut chunk = [0f32; CHUNK];
            chunk[..end - start].copy_from_slice(&samples[start..end]);
            let mut window = [0f32; CONTEXT + CHUNK];
            window[..CONTEXT].copy_from_slice(&context);
            window[CONTEXT..].copy_from_slice(&chunk);
            context.copy_from_slice(&chunk[CHUNK - CONTEXT..]);

            let prob = self.speech_prob(&window)?;

            match open_start {
                None => {
                    if prob >= PROB_START {
                        open_start = Some(start);
                        quiet_from = None;
                    }
                }
                Some(speech_start) => {
                    if prob < PROB_END {
                        let quiet_start = *quiet_from.get_or_insert(start);
                        if end - quiet_start >= END_SILENCE {
                            raw.push(speech_start..quiet_start);
                            open_start = None;
                            quiet_from = None;
                        }
                    } else {
                        quiet_from = None;
                    }
                }
            }
        }
        if let Some(speech_start) = open_start {
            raw.push(speech_start..samples.len());
        }

        let mut spans: Vec<Range<usize>> = Vec::new();
        for span in raw.into_iter().filter(|s| s.end - s.start >= MIN_SPEECH) {
            let start = span.start.saturating_sub(PAD);
            let end = (span.end + PAD).min(samples.len());
            match spans.last_mut() {
                Some(prev) if start <= prev.end => prev.end = prev.end.max(end),
                _ => spans.push(start..end),
            }
        }
        Ok(spans)
    }
}

/// Concatenated audio from the first span start to the last span end; empty when there are none.
pub fn trim_to_speech(samples: &[f32], spans: &[Range<usize>]) -> Vec<f32> {
    let (Some(first), Some(last)) = (spans.first(), spans.last()) else {
        return Vec::new();
    };
    let start = first.start.min(samples.len());
    let end = last.end.min(samples.len()).max(start);
    samples[start..end].to_vec()
}

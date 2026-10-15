//! Parakeet TDT 0.6B v3 through ONNX Runtime: NeMo log-mel preprocessor, Conformer
//! encoder, and token-and-duration greedy decoding of the joint network.

use std::path::Path;

use anyhow::{Context, Result, bail};
use ndarray::{Array1, Array2, Array3, Axis, Ix3, s};
use openflow_core::engine::{SAMPLE_RATE, SpeechToText, SttHints};

use crate::ort_env;

/// LSTM state of the TDT prediction network: [2, 1, 640].
const STATE: (usize, usize, usize) = (2, 1, 640);
/// Tokens per frame before the decoder is forced to move on.
const MAX_SYMBOLS_PER_FRAME: usize = 10;
/// Duration logits follow the token logits; argmax index == duration in encoder frames.
const DURATION_BINS: usize = 5;

pub struct Parakeet {
    preprocessor: ort::session::Session,
    encoder: ort::session::Session,
    decoder: ort::session::Session,
    vocab: Vec<String>,
    blank: i32,
}

impl Parakeet {
    pub fn load(model_dir: &Path) -> Result<Self> {
        let encoder = find_model(model_dir, "encoder-model")?;
        let decoder = find_model(model_dir, "decoder_joint-model")?;
        let preprocessor = model_dir.join("nemo128.onnx");
        if !preprocessor.is_file() {
            bail!("missing {}", preprocessor.display());
        }
        let (vocab, blank) = load_vocab(model_dir)?;
        Ok(Self {
            preprocessor: ort_env::session(&preprocessor)?,
            encoder: ort_env::session(&encoder)?,
            decoder: ort_env::session(&decoder)?,
            vocab,
            blank,
        })
    }

    /// Greedy TDT decode over the encoder frames `0..limit`.
    fn decode(&mut self, encoded: &Array3<f32>, limit: usize) -> Result<Vec<i32>> {
        let vocab_size = self.vocab.len();
        let mut state1 = Array3::<f32>::zeros(STATE);
        let mut state2 = Array3::<f32>::zeros(STATE);
        let mut tokens: Vec<i32> = Vec::new();
        let mut last = self.blank;
        let mut t = 0usize;
        let mut symbols = 0usize;

        while t < limit {
            let frame = encoded.slice(s![0, .., t]).to_owned();
            let encoder_outputs = frame.insert_axis(Axis(0)).insert_axis(Axis(2));
            let targets = Array2::from_shape_vec((1, 1), vec![last])?;
            let target_length = Array1::from_vec(vec![1i32]);

            let outputs = self.decoder.run(ort::inputs![
                "encoder_outputs" => ort::value::TensorRef::from_array_view(encoder_outputs.view())?,
                "targets" => ort::value::TensorRef::from_array_view(targets.view())?,
                "target_length" => ort::value::TensorRef::from_array_view(target_length.view())?,
                "input_states_1" => ort::value::TensorRef::from_array_view(state1.view())?,
                "input_states_2" => ort::value::TensorRef::from_array_view(state2.view())?,
            ])?;

            let logits: Vec<f32> = outputs
                .get("outputs")
                .context("Parakeet decoder output `outputs` missing")?
                .try_extract_array::<f32>()?
                .iter()
                .copied()
                .collect();
            let token = argmax(logits.get(..vocab_size).unwrap_or(&[]));
            // The duration bins are the durations themselves, so the argmax index is the advance.
            let duration = argmax(logits.get(vocab_size..vocab_size + DURATION_BINS).unwrap_or(&[]));

            if token == self.blank {
                t += duration.max(1) as usize;
                symbols = 0;
            } else {
                tokens.push(token);
                last = token;
                state1 = extract_state(&outputs, "output_states_1")?;
                state2 = extract_state(&outputs, "output_states_2")?;
                if duration == 0 {
                    symbols += 1;
                    if symbols >= MAX_SYMBOLS_PER_FRAME {
                        t += 1;
                        symbols = 0;
                    }
                } else {
                    t += duration as usize;
                    symbols = 0;
                }
            }
        }
        Ok(tokens)
    }

    fn detokenize(&self, tokens: &[i32]) -> String {
        let mut text = String::new();
        for &token in tokens {
            if let Some(piece) = self.vocab.get(token as usize) {
                text.push_str(piece);
            }
        }
        text.replace('\u{2581}', " ").trim().to_string()
    }
}

impl SpeechToText for Parakeet {
    fn transcribe(&mut self, samples: &[f32], _hints: &SttHints) -> Result<String> {
        if samples.len() < SAMPLE_RATE as usize / 10 {
            return Ok(String::new());
        }

        let waveforms = Array2::from_shape_vec((1, samples.len()), samples.to_vec())?;
        let waveforms_lens = Array1::from_vec(vec![samples.len() as i64]);
        let (features, features_lens) = {
            let outputs = self.preprocessor.run(ort::inputs![
                "waveforms" => ort::value::TensorRef::from_array_view(waveforms.view())?,
                "waveforms_lens" => ort::value::TensorRef::from_array_view(waveforms_lens.view())?,
            ])?;
            (extract_f32(&outputs, "features")?, extract_i64(&outputs, "features_lens")?)
        };

        let (encoded, encoded_lens) = {
            let outputs = self.encoder.run(ort::inputs![
                "audio_signal" => ort::value::TensorRef::from_array_view(features.view())?,
                "length" => ort::value::TensorRef::from_array_view(features_lens.view())?,
            ])?;
            (extract_f32(&outputs, "outputs")?, extract_i64(&outputs, "encoded_lengths")?)
        };

        let limit = (encoded_lens.first().copied().unwrap_or(0).max(0) as usize)
            .min(encoded.shape()[2]);
        let tokens = self.decode(&encoded, limit)?;
        Ok(self.detokenize(&tokens))
    }
}

fn find_model(model_dir: &Path, stem: &str) -> Result<std::path::PathBuf> {
    let quantized = model_dir.join(format!("{stem}.int8.onnx"));
    if quantized.is_file() {
        return Ok(quantized);
    }
    let regular = model_dir.join(format!("{stem}.onnx"));
    if regular.is_file() {
        return Ok(regular);
    }
    bail!("missing {} (or {})", quantized.display(), regular.display())
}

fn extract_f32(outputs: &ort::session::SessionOutputs<'_>, name: &str) -> Result<Array3<f32>> {
    Ok(outputs
        .get(name)
        .with_context(|| format!("model output `{name}` missing"))?
        .try_extract_array::<f32>()?
        .to_owned()
        .into_dimensionality::<Ix3>()?)
}

fn extract_i64(outputs: &ort::session::SessionOutputs<'_>, name: &str) -> Result<Array1<i64>> {
    Ok(outputs
        .get(name)
        .with_context(|| format!("model output `{name}` missing"))?
        .try_extract_array::<i64>()?
        .to_owned()
        .into_dimensionality::<ndarray::Ix1>()?)
}

fn extract_state(outputs: &ort::session::SessionOutputs<'_>, name: &str) -> Result<Array3<f32>> {
    Ok(outputs
        .get(name)
        .with_context(|| format!("model output `{name}` missing"))?
        .try_extract_array::<f32>()?
        .to_owned()
        .into_dimensionality::<Ix3>()?)
}

fn argmax(values: &[f32]) -> i32 {
    let mut best = 0usize;
    for (index, value) in values.iter().enumerate() {
        if *value > values[best] {
            best = index;
        }
    }
    best as i32
}

/// Reads `vocab.txt` (`token id` per line). `<blk>` is the blank id, `\u{2581}` a word start.
fn load_vocab(model_dir: &Path) -> Result<(Vec<String>, i32)> {
    let path = model_dir.join("vocab.txt");
    let content = std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;

    let mut entries: Vec<(usize, String)> = Vec::new();
    let mut blank: Option<i32> = None;
    for line in content.lines() {
        let Some((token, id)) = line.rsplit_once(' ') else {
            continue;
        };
        let Ok(id) = id.trim().parse::<usize>() else {
            continue;
        };
        if token == "<blk>" {
            blank = Some(id as i32);
        }
        entries.push((id, token.to_string()));
    }

    let blank = blank.context("vocab.txt has no <blk> token")?;
    let size = entries.iter().map(|(id, _)| *id).max().unwrap_or(0) + 1;
    let mut vocab = vec![String::new(); size];
    for (id, token) in entries {
        vocab[id] = token;
    }
    Ok((vocab, blank))
}

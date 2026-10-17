//! Whisper through candle: the multilingual fallback for languages Parakeet
//! does not cover. Mel front-end and the greedy decoder loop follow the
//! reference implementation; the model code is candle's (MIT / Apache-2.0,
//! github.com/huggingface/candle, examples/whisper).

use std::path::Path;

use anyhow::{Context, anyhow, bail};
use candle_core::{DType, Device, IndexOp, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::whisper::{self, audio, model::Whisper as WhisperModel};
use openflow_core::engine::{SpeechToText, SttHints, SAMPLE_RATE};

use crate::mel;

/// 30 s window, the length Whisper was trained on.
const CHUNK_SAMPLES: usize = 30 * SAMPLE_RATE as usize;
/// Decoder steps per 30 s window; the reference uses `max_length = 448 / 2`.
const MAX_TOKENS: usize = 224;

pub struct Whisper {
    model: WhisperModel,
    config: whisper::Config,
    filters: Vec<f32>,
    tokenizer: tokenizers::Tokenizer,
    device: Device,
    sot: u32,
    eot: u32,
    transcribe: u32,
    no_timestamps: u32,
    start_of_prev: Option<u32>,
}

impl Whisper {
    pub fn load(dir: &Path) -> anyhow::Result<Self> {
        let device = crate::pick_device()?;
        let config: whisper::Config = serde_json::from_str(
            &std::fs::read_to_string(dir.join("config.json")).context("read config.json")?,
        )
        .context("parse config.json")?;
        let weights = dir.join("model.safetensors");
        let vb = unsafe {
            VarBuilder::from_mmaped_safetensors(&[weights], whisper::DTYPE, &device)
                .context("map model.safetensors")?
        };
        let model = WhisperModel::load(&vb, config.clone()).context("load Whisper weights")?;
        let tokenizer = tokenizers::Tokenizer::from_file(dir.join("tokenizer.json"))
            .map_err(|e| anyhow!("load tokenizer: {e}"))?;
        let id = |token: &str| -> anyhow::Result<u32> {
            tokenizer
                .token_to_id(token)
                .ok_or_else(|| anyhow!("tokenizer has no {token}"))
        };
        let filters = mel::slaney_filters(config.num_mel_bins);
        Ok(Self {
            model,
            config,
            filters,
            sot: id(whisper::SOT_TOKEN)?,
            eot: id(whisper::EOT_TOKEN)?,
            transcribe: id(whisper::TRANSCRIBE_TOKEN)?,
            no_timestamps: id(whisper::NO_TIMESTAMPS_TOKEN)?,
            start_of_prev: tokenizer.token_to_id("<|startofprev|>"),
            tokenizer,
            device,
        })
    }

    fn token(&self, token: &str) -> Option<u32> {
        self.tokenizer.token_to_id(token)
    }

    /// Language token for `code` ("hi" -> `<|hi|>`), or the detected language by
    /// running one decoder step over the audio, as the reference does.
    fn language_token(&mut self, code: Option<&str>, features: &Tensor) -> anyhow::Result<u32> {
        const LANGUAGES: &[&str] = &[
            "en", "zh", "de", "es", "ru", "ko", "fr", "ja", "pt", "tr", "pl", "ca", "nl", "ar", "sv", "it",
            "id", "hi", "fi", "vi", "he", "uk", "el", "ms", "cs", "ro", "da", "hu", "ta", "no", "th", "ur",
            "hr", "bg", "lt", "la", "mi", "ml", "cy", "sk", "te", "fa", "lv", "bn", "sr", "az", "sl", "kn",
            "et", "mk", "br", "eu", "is", "hy", "ne", "mn", "bs", "kk", "sq", "sw", "gl", "mr", "pa", "si",
            "km", "sn", "yo", "so", "af", "oc", "ka", "be", "tg", "sd", "gu", "am", "yi", "lo", "uz", "fo",
            "ht", "ps", "tk", "nn", "mt", "sa", "lb", "my", "bo", "tl", "mg", "as", "tt", "haw", "ln", "ha",
            "ba", "jw", "su", "yue",
        ];
        if let Some(code) = code {
            let code = code.split(['-', '_']).next().unwrap_or(code);
            if !LANGUAGES.contains(&code) {
                bail!("Whisper does not support language {code:?}");
            }
            return self
                .token(&format!("<|{code}|>"))
                .ok_or_else(|| anyhow!("tokenizer has no <|{code}|> token"));
        }
        let tokens = Tensor::new(&[self.sot], &self.device)?.unsqueeze(0)?;
        // The language tokens live above `<|endoftext|>`, so they must not go
        // through the special-token mask `next_logits` applies.
        let logits = self.raw_logits(&tokens, features, true)?;
        let mut best = (0u32, f32::NEG_INFINITY);
        for code in LANGUAGES.iter() {
            let Some(id) = self.token(&format!("<|{code}|>")) else { continue };
            let score: f32 = logits.i(id as usize)?.to_scalar()?;
            if score > best.1 {
                best = (id, score);
            }
        }
        if best.1 == f32::NEG_INFINITY {
            bail!("no language tokens in the tokenizer");
        }
        log::debug!("whisper detected {}", best.0);
        Ok(best.0)
    }

    /// Hotwords become Whisper's "previous context" prompt so the model prefers
    /// the dictionary's spelling.
    fn prompt_tokens(&self, hotwords: &[String]) -> Vec<u32> {
        let (Some(start), false) = (self.start_of_prev, hotwords.is_empty()) else { return vec![] };
        let text = hotwords.join(", ");
        let Ok(encoding) = self.tokenizer.encode(text, false) else { return vec![] };
        let mut ids = vec![start];
        ids.extend(encoding.get_ids().iter().copied().take(200));
        ids
    }

    fn transcribe_chunk(&mut self, samples: &[f32], hints: &SttHints) -> anyhow::Result<String> {
        let mut padded = vec![0f32; CHUNK_SAMPLES];
        let take = samples.len().min(CHUNK_SAMPLES);
        padded[..take].copy_from_slice(&samples[..take]);
        // candle's front-end pads the chunk to a multiple of half a chunk, so
        // the row stride comes from the returned length, not the sample count;
        // the first 3000 frames are the ones Whisper expects for 30 s of audio.
        let mel = audio::pcm_to_mel(&self.config, &padded, &self.filters);
        let frames = mel.len() / self.config.num_mel_bins;
        let features = Tensor::from_vec(mel, (self.config.num_mel_bins, frames), &self.device)?
            .narrow(1, 0, whisper::N_FRAMES)?
            .unsqueeze(0)?
            .to_dtype(DType::F32)?;
        let features = self.model.encoder.forward(&features, true)?;
        let language = self.language_token(hints.language.as_deref(), &features)?;
        let mut tokens = self.prompt_tokens(&hints.hotwords);
        tokens.extend([self.sot, language, self.transcribe, self.no_timestamps]);

        let mut generated = Vec::new();
        let (mut logprob, mut steps) = (0f32, 0usize);
        for step in 0..MAX_TOKENS {
            // The whole prefix goes in every step: candle's decoder caches only
            // the cross-attention keys, and the position embeddings come from the
            // sequence it is given.
            let input = Tensor::new(tokens.as_slice(), &self.device)?.unsqueeze(0)?;
            let logits = self.next_logits(&input, &features, step == 0)?;
            let probs = candle_nn::ops::log_softmax(&logits, candle_core::D::Minus1)?;
            let next = probs.argmax(candle_core::D::Minus1)?.to_scalar::<u32>()?;
            if next == self.eot {
                break;
            }
            logprob += probs.i(next as usize)?.to_scalar::<f32>()?;
            steps += 1;
            generated.push(next);
            tokens.push(next);
        }
        // The reference gives up on a window whose tokens the model did not
        // believe in, which is what keeps silence from hallucinating.
        if steps > 0 && logprob / steps as f32 <= whisper::LOGPROB_THRESHOLD as f32 {
            log::debug!("whisper dropped a window: avg logprob {:.2}", logprob / steps as f32);
            return Ok(String::new());
        }
        self.decode(&generated)
    }

    /// Logits for the next token after `tokens`, special tokens suppressed.
    fn next_logits(&mut self, tokens: &Tensor, features: &Tensor, flush: bool) -> anyhow::Result<Tensor> {
        let raw = self.raw_logits(tokens, features, flush)?;
        suppress_specials(&raw, self.eot)
    }

    fn raw_logits(&mut self, tokens: &Tensor, features: &Tensor, flush: bool) -> anyhow::Result<Tensor> {
        let hidden = self.model.decoder.forward(tokens, features, flush)?;
        let last = hidden.dim(1)? - 1;
        let hidden = hidden.narrow(1, last, 1)?;
        Ok(self.model.decoder.final_linear(&hidden)?.squeeze(0)?.squeeze(0)?)
    }

    fn decode(&self, tokens: &[u32]) -> anyhow::Result<String> {
        let text = self
            .tokenizer
            .decode(tokens, false)
            .map_err(|e| anyhow!("detokenize: {e}"))?;
        Ok(text.trim().to_string())
    }
}

/// Whisper must not emit special tokens (`<|...|>`, timestamps) after the
/// prompt; `eot` itself stays legal so the loop can stop on it.
fn suppress_specials(logits: &Tensor, eot: u32) -> anyhow::Result<Tensor> {
    let n = logits.dim(0)?;
    let values = logits.to_vec1::<f32>()?;
    let masked: Vec<f32> = values
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let special = i as u32 > eot;
            if special { f32::NEG_INFINITY } else { *v }
        })
        .collect();
    debug_assert_eq!(masked.len(), n);
    Ok(Tensor::from_vec(masked, n, logits.device())?)
}

impl SpeechToText for Whisper {
    fn transcribe(&mut self, samples: &[f32], hints: &SttHints) -> anyhow::Result<String> {
        let mut parts = Vec::new();
        for (index, chunk) in samples.chunks(CHUNK_SAMPLES).enumerate() {
            let text = self
                .transcribe_chunk(chunk, hints)
                .with_context(|| format!("whisper chunk {index}"))?;
            if !text.is_empty() {
                parts.push(text);
            }
        }
        Ok(parts.join(" ").trim().to_string())
    }
}

//! Quantized GGUF text generation through candle, for local prompt cleanup.

use std::path::Path;

use anyhow::{anyhow, bail, Context, Result};
use candle_core::quantized::gguf_file;
use candle_core::{Device, Tensor};
use candle_transformers::generation::{LogitsProcessor, Sampling};
use candle_transformers::models::{quantized_gemma3, quantized_llama, quantized_qwen3};
use candle_transformers::utils::apply_repeat_penalty;
use openflow_core::engine::{ChatPrompt, GenOptions, TextGenerator};

/// Repeat penalty applied over the last `REPEAT_WINDOW` tokens.
const REPEAT_PENALTY: f32 = 1.1;
const REPEAT_WINDOW: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    Qwen3,
    Gemma3,
    Llama,
}

impl Family {
    fn from_architecture(arch: &str) -> Result<Self> {
        match arch {
            "qwen3" => Ok(Self::Qwen3),
            "gemma3" => Ok(Self::Gemma3),
            "llama" => Ok(Self::Llama),
            other => bail!("unsupported GGUF architecture {other:?} (want qwen3, gemma3 or llama)"),
        }
    }

    fn stop_tokens(self) -> &'static [&'static str] {
        match self {
            Self::Qwen3 => &["<|im_end|>", "<|endoftext|>"],
            Self::Gemma3 => &["<end_of_turn>", "<eos>"],
            Self::Llama => &["<|eot_id|>", "<|end_of_text|>"],
        }
    }
}

#[derive(Clone)]
enum Weights {
    Qwen3(Box<quantized_qwen3::ModelWeights>),
    Gemma3(Box<quantized_gemma3::ModelWeights>),
    Llama(Box<quantized_llama::ModelWeights>),
}

/// Holds two cheap clones of the weights (candle tensors are shared,
/// immutable buffers): `pristine` with an empty KV cache, and `prefix` with
/// the cache filled for the last system prompt. Cleanup reuses one fixed
/// system prompt, so its prefill runs once per session instead of per call.
pub struct LocalLlm {
    pristine: Weights,
    prefix: Option<(String, usize, Weights)>,
    tokenizer: tokenizers::Tokenizer,
    device: Device,
    family: Family,
    stop_ids: Vec<u32>,
}

impl LocalLlm {
    pub fn load(gguf: &Path, tokenizer_json: &Path) -> Result<Self> {
        let device = crate::pick_device()?;
        let mut file =
            std::fs::File::open(gguf).with_context(|| format!("open {}", gguf.display()))?;
        let ct = gguf_file::Content::read(&mut file).context("read GGUF header")?;
        let arch = ct
            .metadata
            .get("general.architecture")
            .and_then(|v| v.to_string().ok())
            .ok_or_else(|| anyhow!("GGUF is missing general.architecture"))?
            .clone();
        let family = Family::from_architecture(&arch)?;
        let weights = read_weights(family, ct, &mut file, &device)?;
        let tokenizer = tokenizers::Tokenizer::from_file(tokenizer_json)
            .map_err(|e| anyhow!("load tokenizer {}: {e}", tokenizer_json.display()))?;
        let stop_ids = family
            .stop_tokens()
            .iter()
            .filter_map(|s| tokenizer.token_to_id(s))
            .collect();
        Ok(Self {
            pristine: weights,
            prefix: None,
            tokenizer,
            device,
            family,
            stop_ids,
        })
    }

    pub fn family(&self) -> Family {
        self.family
    }

    /// Chat-template text for this family, including the assistant turn opener.
    pub fn render(&self, prompt: &ChatPrompt) -> String {
        let (head, tail) = self.template(prompt);
        head + &tail
    }

    /// The template split where the user's text begins: everything before it
    /// depends only on the system prompt and can be cached.
    fn template(&self, prompt: &ChatPrompt) -> (String, String) {
        let (system, user) = (prompt.system.as_str(), prompt.user.as_str());
        match self.family {
            Family::Qwen3 => (
                format!("<|im_start|>system\n{system}<|im_end|>\n<|im_start|>user\n"),
                format!("{user}<|im_end|>\n<|im_start|>assistant\n<think>\n\n</think>\n\n"),
            ),
            Family::Gemma3 => (
                format!("<bos><start_of_turn>user\n{system}\n\n"),
                format!("{user}<end_of_turn>\n<start_of_turn>model\n"),
            ),
            Family::Llama => (
                format!(
                    "<|begin_of_text|><|start_header_id|>system<|end_header_id|>\n\n{system}<|eot_id|>\
                     <|start_header_id|>user<|end_header_id|>\n\n"
                ),
                format!("{user}<|eot_id|><|start_header_id|>assistant<|end_header_id|>\n\n"),
            ),
        }
    }

    fn encode(&self, text: &str) -> Result<Vec<u32>> {
        Ok(self.tokenizer.encode(text, false).map_err(|e| anyhow!("tokenize prompt: {e}"))?.get_ids().to_vec())
    }

    /// Weights whose KV cache holds `head`, plus its token count.
    fn weights_after(&mut self, head: &str) -> Result<(Weights, usize)> {
        if let Some((cached, len, weights)) = &self.prefix
            && cached == head
        {
            return Ok((weights.clone(), *len));
        }
        let tokens = self.encode(head)?;
        let mut weights = self.pristine.clone();
        let input = Tensor::from_slice(&tokens, (1, tokens.len()), &self.device)?;
        forward(&mut weights, &input, 0)?;
        self.prefix = Some((head.to_string(), tokens.len(), weights.clone()));
        Ok((weights, tokens.len()))
    }
}

fn forward(weights: &mut Weights, input: &Tensor, offset: usize) -> Result<Tensor> {
    Ok(match weights {
        Weights::Qwen3(w) => w.forward(input, offset)?,
        Weights::Gemma3(w) => w.forward(input, offset)?,
        Weights::Llama(w) => w.forward(input, offset)?,
    })
}

fn read_weights(
    family: Family,
    ct: gguf_file::Content,
    file: &mut std::fs::File,
    device: &Device,
) -> Result<Weights> {
    Ok(match family {
        Family::Qwen3 => {
            Weights::Qwen3(Box::new(quantized_qwen3::ModelWeights::from_gguf(ct, file, device)?))
        }
        Family::Gemma3 => {
            Weights::Gemma3(Box::new(quantized_gemma3::ModelWeights::from_gguf(ct, file, device)?))
        }
        Family::Llama => {
            Weights::Llama(Box::new(quantized_llama::ModelWeights::from_gguf(ct, file, device)?))
        }
    })
}

impl TextGenerator for LocalLlm {
    fn generate(&mut self, prompt: &ChatPrompt, opts: &GenOptions) -> Result<String> {
        let (head, tail) = self.template(prompt);
        let (mut weights, offset) = self.weights_after(&head)?;
        let tail_tokens = self.encode(&tail)?;
        if tail_tokens.is_empty() {
            bail!("prompt tokenized to nothing");
        }
        let mut sampler = if opts.temperature <= 0.0 {
            LogitsProcessor::from_sampling(42, Sampling::ArgMax)
        } else {
            LogitsProcessor::from_sampling(42, Sampling::All { temperature: opts.temperature })
        };

        let started = std::time::Instant::now();
        let input = Tensor::from_slice(&tail_tokens, (1, tail_tokens.len()), &self.device)?;
        let mut logits = forward(&mut weights, &input, offset)?.squeeze(0)?;
        let mut tokens = tail_tokens;

        let mut generated = Vec::new();
        for step in 0..opts.max_tokens {
            let window = &tokens[tokens.len().saturating_sub(REPEAT_WINDOW)..];
            let penalized = apply_repeat_penalty(&logits, REPEAT_PENALTY, window)?;
            let next = sampler.sample(&penalized)?;
            if self.stop_ids.contains(&next) {
                break;
            }
            generated.push(next);
            tokens.push(next);
            if step + 1 == opts.max_tokens {
                break;
            }
            let input = Tensor::from_slice(&[next], (1, 1), &self.device)?;
            logits = forward(&mut weights, &input, offset + tokens.len() - 1)?.squeeze(0)?;
        }

        let elapsed = started.elapsed().as_secs_f64();
        log::debug!(
            "llm: {} tokens in {:.2}s ({:.1} tok/s)",
            generated.len(),
            elapsed,
            generated.len() as f64 / elapsed.max(1e-6)
        );

        let out = self
            .tokenizer
            .decode(&generated, true)
            .map_err(|e| anyhow!("detokenize: {e}"))?;
        Ok(out.trim().to_string())
    }
}

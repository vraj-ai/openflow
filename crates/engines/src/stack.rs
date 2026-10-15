//! Builds the controller's `Engines` from settings and installed models.

use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use openflow_core::catalog::{self, ModelSpec};
use openflow_core::controller::Engines;
use openflow_core::settings::Settings;

use crate::llm::LocalLlm;
use crate::parakeet::Parakeet;
use crate::vad::SileroVad;
use crate::whisper::Whisper;

/// Where models live: `$OPENFLOW_MODELS`, else `<data dir>/models`.
pub fn models_root(data_dir: &Path) -> PathBuf {
    std::env::var_os("OPENFLOW_MODELS").map(PathBuf::from).unwrap_or_else(|| data_dir.join("models"))
}

/// Resolves a catalog id to its directory, accepting a legacy dev layout
/// (`models/parakeet-v3`) through `OPENFLOW_MODEL_DIR_<ID>` overrides.
pub fn installed(id: &str, root: &Path) -> anyhow::Result<(&'static ModelSpec, PathBuf)> {
    let spec = catalog::get(id).with_context(|| format!("unknown model id {id}"))?;
    let var = format!("OPENFLOW_MODEL_DIR_{}", id.to_uppercase().replace(['-', '.'], "_"));
    if let Some(dir) = std::env::var_os(&var) {
        return Ok((spec, PathBuf::from(dir)));
    }
    if !spec.is_installed(root) {
        bail!("model {id} is not downloaded; run `openflow models download {id}` or use Settings > Models");
    }
    Ok((spec, spec.dir(root)))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Load {
    pub vad: bool,
    pub llm: bool,
    pub multilingual: bool,
}

pub fn load(settings: &Settings, root: &Path, what: Load) -> anyhow::Result<Engines> {
    crate::ort_env::init()?;
    let (_, stt_dir) = installed(&settings.stt.model, root)?;
    let stt = Box::new(Parakeet::load(&stt_dir)?);
    let vad = match (what.vad && settings.stt.vad, installed("silero-vad", root)) {
        (true, Ok((_, dir))) => Some(Box::new(SileroVad::load(&dir.join("silero_vad.onnx"))?) as _),
        (true, Err(e)) => {
            log::warn!("VAD disabled: {e:#}");
            None
        }
        _ => None,
    };
    let llm = match (what.llm && settings.cleanup.llm_enabled, installed(&settings.cleanup.llm_model, root)) {
        (true, Ok((_, dir))) => Some(Box::new(LocalLlm::load(&dir.join("model.gguf"), &dir.join("tokenizer.json"))?) as _),
        (true, Err(e)) => {
            log::warn!("LLM cleanup disabled: {e:#}");
            None
        }
        _ => None,
    };
    let multilingual = match (what.multilingual, installed(&settings.stt.multilingual_model, root)) {
        (true, Ok((_, dir))) => Some(Box::new(Whisper::load(&dir)?) as _),
        (true, Err(e)) => {
            log::info!("multilingual fallback unavailable: {e:#}");
            None
        }
        _ => None,
    };
    Ok(Engines { stt, multilingual, vad, llm })
}

//! Local inference engines implementing the `openflow_core::engine` traits.

pub mod audio;
pub mod download;
pub mod llm;
pub mod ort_env;
pub mod parakeet;
pub mod stack;
pub mod vad;
pub mod whisper;

/// Which accelerators this build was compiled with, and whether this machine has
/// one. Both matter: a CUDA build on a box without a driver still runs on CPU.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Backends {
    pub cuda_built: bool,
    pub cuda_available: bool,
    pub metal_built: bool,
    pub metal_available: bool,
    pub coreml_built: bool,
}

pub fn backends() -> Backends {
    #[cfg(feature = "cuda")]
    let cuda_available = candle_core::utils::cuda_is_available();
    #[cfg(not(feature = "cuda"))]
    let cuda_available = false;
    #[cfg(feature = "metal")]
    let metal_available = candle_core::utils::metal_is_available();
    #[cfg(not(feature = "metal"))]
    let metal_available = false;
    Backends {
        cuda_built: cfg!(feature = "cuda"),
        cuda_available,
        metal_built: cfg!(feature = "metal"),
        metal_available,
        coreml_built: cfg!(feature = "coreml"),
    }
}

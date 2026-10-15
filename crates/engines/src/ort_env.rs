//! Process-wide ONNX Runtime setup: load the shared library once, then build sessions from it.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use anyhow::{Context, Result, anyhow};
use ort::environment::Environment;
use ort::ep::ExecutionProviderDispatch;
use ort::session::Session;
use ort::session::builder::GraphOptimizationLevel;

static ENV: OnceLock<Environment> = OnceLock::new();
static INIT: OnceLock<Result<(), String>> = OnceLock::new();

const DYLIB_NAMES: [&str; 3] = ["libonnxruntime.so", "libonnxruntime.dylib", "onnxruntime.dll"];

/// Loads ONNX Runtime and builds the global environment. Safe to call repeatedly.
pub fn init() -> Result<()> {
    let result = INIT.get_or_init(|| {
        let path = match dylib_path() {
            Ok(path) => path,
            Err(err) => return Err(format!("{err:#}")),
        };
        let builder = match ort::init_from(&path) {
            Ok(builder) => builder,
            Err(err) => {
                return Err(format!("failed to load ONNX Runtime from {}: {err}", path.display()));
            }
        };
        let env = match builder.with_execution_providers(execution_providers()).build() {
            Ok(env) => env,
            Err(err) => return Err(format!("failed to create ONNX Runtime environment: {err}")),
        };
        // The environment is only reachable while a handle to it lives, so keep it for the process.
        let _ = ENV.set(env);
        Ok(())
    });
    result.clone().map_err(|err| anyhow!(err))
}

/// Opens an optimized session for the model at `path`.
pub fn session(path: &Path) -> Result<Session> {
    init()?;
    let env = ENV.get().context("ONNX Runtime environment was not initialized")?;
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1).min(8);
    let mut builder = Session::builder(env)
        .map_err(to_anyhow)?
        .with_optimization_level(GraphOptimizationLevel::Level3)
        .map_err(to_anyhow)?
        .with_intra_threads(threads)
        .map_err(to_anyhow)?;
    let providers = execution_providers();
    if !providers.is_empty() {
        builder = builder.with_execution_providers(providers).map_err(to_anyhow)?;
    }
    Ok(builder.commit_from_file(path)?)
}

/// `ort::Error<SessionBuilder>` is not `Send`/`Sync`, so it cannot go straight into `anyhow`.
fn to_anyhow(err: impl std::fmt::Display) -> anyhow::Error {
    anyhow!("{err}")
}

fn execution_providers() -> Vec<ExecutionProviderDispatch> {
    #[allow(unused_mut)]
    let mut providers: Vec<ExecutionProviderDispatch> = Vec::new();
    #[cfg(feature = "coreml")]
    providers.push(ort::ep::CoreML::default().build());
    #[cfg(feature = "cuda")]
    providers.push(ort::ep::CUDA::default().build());
    providers
}

fn dylib_path() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("ORT_DYLIB_PATH") {
        return Ok(PathBuf::from(path));
    }
    if let Some(dir) = std::env::current_exe().ok().and_then(|exe| exe.parent().map(Path::to_path_buf))
        && let Some(path) = DYLIB_NAMES.iter().map(|name| dir.join(name)).find(|path| path.is_file())
    {
        return Ok(path);
    }
    anyhow::bail!(
        "ONNX Runtime shared library not found: set ORT_DYLIB_PATH to {} (or place one next to the executable)",
        DYLIB_NAMES.join(", ")
    )
}

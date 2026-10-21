//! `openflow doctor` must report whether ONNX Runtime actually loads, not just whether the path exists.

use std::path::{Path, PathBuf};
use std::process::Command;

fn scratch(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("doctor-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn doctor(ort: &Path, scratch: &Path) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_openflow"))
        .arg("doctor")
        .env("ORT_DYLIB_PATH", ort)
        .env("OPENFLOW_HOME", scratch.join("home"))
        .env("OPENFLOW_MODELS", scratch.join("models"))
        .output()
        .unwrap();
    assert!(out.status.success(), "doctor exited {:?}: {}", out.status.code(), String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout).unwrap()
}

#[test]
fn doctor_reports_a_broken_onnxruntime_as_failed() {
    let dir = scratch("broken");
    let fake = dir.join("libonnxruntime.so");
    std::fs::write(&fake, b"not a shared library").unwrap();
    let stdout = doctor(&fake, &dir);
    let prefix = format!("onnxruntime: {} (failed: ", fake.display());
    assert!(
        stdout.lines().any(|l| l.starts_with(&prefix) && l.ends_with(')')),
        "no line starting with {prefix:?} in:\n{stdout}"
    );
}

#[test]
#[ignore = "needs ORT_DYLIB_PATH pointing at a real libonnxruntime"]
fn doctor_reports_a_working_onnxruntime_as_loads() {
    let real = std::env::var_os("ORT_DYLIB_PATH")
        .map(PathBuf::from)
        .filter(|p| p.is_file())
        .expect("ORT_DYLIB_PATH must point at a real libonnxruntime file");
    let dir = scratch("working");
    let stdout = doctor(&real, &dir);
    let want = format!("onnxruntime: {} (loads)", real.display());
    assert!(stdout.lines().any(|l| l == want), "no line {want:?} in:\n{stdout}");
}

//! Downloads catalog models from Hugging Face with resume and SHA-256 checks.
//! This is the only network code in OpenFlow.

use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;

use anyhow::{Context, bail};
use openflow_core::catalog::{ModelFile, ModelSpec};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    pub done: u64,
    pub total: u64,
}

/// Downloads every missing file of `spec` into `<models_root>/<id>/`.
/// Partial files (`*.part`) are resumed with an HTTP Range request, and a file
/// only gets its final name after its size and hash check out, so a crash or
/// a cancelled download never leaves a file that looks installed.
pub fn download(
    spec: &ModelSpec,
    models_root: &Path,
    on_progress: &mut dyn FnMut(Progress),
    cancelled: &dyn Fn() -> bool,
) -> anyhow::Result<()> {
    let dir = spec.dir(models_root);
    std::fs::create_dir_all(&dir)?;
    let total = spec.download_size();
    let mut done_before = 0u64;
    for file in &spec.files {
        let target = dir.join(&file.name);
        if std::fs::metadata(&target).is_ok_and(|m| m.len() == file.size) {
            done_before += file.size;
            on_progress(Progress { done: done_before, total });
            continue;
        }
        fetch(file, &target, &mut |n| on_progress(Progress { done: done_before + n, total }), cancelled)
            .with_context(|| format!("downloading {}", file.url))?;
        done_before += file.size;
    }
    Ok(())
}

fn fetch(file: &ModelFile, target: &Path, on_bytes: &mut dyn FnMut(u64), cancelled: &dyn Fn() -> bool) -> anyhow::Result<()> {
    if !file.url.starts_with("https://huggingface.co/") {
        bail!("refusing to download from outside huggingface.co: {}", file.url);
    }
    let part = target.with_extension(format!(
        "{}part",
        target.extension().map(|e| format!("{}.", e.to_string_lossy())).unwrap_or_default()
    ));
    let mut have = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
    if have > file.size {
        std::fs::remove_file(&part)?;
        have = 0;
    }
    if have < file.size {
        let mut request = ureq::get(&file.url);
        if have > 0 {
            request = request.header("Range", &format!("bytes={have}-"));
        }
        let response = request.call()?;
        if have > 0 && response.status() != 206 {
            have = 0; // server ignored the range; start over
        }
        let mut out = OpenOptions::new().create(true).write(true).append(have > 0).truncate(have == 0).open(&part)?;
        let mut body = response.into_body().into_reader();
        let mut buf = vec![0u8; 1 << 20];
        let mut written = have;
        loop {
            if cancelled() {
                bail!("cancelled");
            }
            let n = body.read(&mut buf)?;
            if n == 0 {
                break;
            }
            out.write_all(&buf[..n])?;
            written += n as u64;
            on_bytes(written);
        }
        out.flush()?;
    }
    let len = std::fs::metadata(&part)?.len();
    if len != file.size {
        bail!("size mismatch: got {len} bytes, expected {}", file.size);
    }
    if let Some(expected) = &file.sha256 {
        let actual = sha256_file(&part)?;
        if &actual != expected {
            std::fs::remove_file(&part)?;
            bail!("checksum mismatch for {}: {actual} != {expected}", file.name);
        }
    }
    std::fs::rename(&part, target)?;
    Ok(())
}

pub fn sha256_file(path: &Path) -> anyhow::Result<String> {
    let mut f = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

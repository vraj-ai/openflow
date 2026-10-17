//! Whisper (multilingual fallback) checks. Model-backed tests are ignored unless
//! run with `--include-ignored` and are skipped when `OPENFLOW_MODELS` (or
//! `<repo>/models`) is absent.

use std::path::{Path, PathBuf};

use openflow_core::engine::{SttHints, SpeechToText};
use openflow_engines::audio::load_wav;
use openflow_engines::whisper::Whisper;

const EXPECTED: &str =
    "mister quilter is the apostle of the middle classes and we are glad to welcome his gospel";

fn models() -> Option<PathBuf> {
    let dir = std::env::var_os("OPENFLOW_MODELS")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../models"));
    dir.is_dir().then_some(dir)
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name)
}

fn words(text: &str) -> Vec<String> {
    text.split_whitespace()
        .map(|word| word.trim_matches(|c: char| !c.is_alphanumeric() && c != '\'').to_lowercase())
        .filter(|word| !word.is_empty())
        .collect()
}

fn wer(reference: &str, hypothesis: &str) -> f64 {
    let (r, h) = (words(reference), words(hypothesis));
    if r.is_empty() {
        return 1.0;
    }
    let mut prev: Vec<usize> = (0..=h.len()).collect();
    let mut cur = vec![0usize; h.len() + 1];
    for (i, a) in r.iter().enumerate() {
        cur[0] = i + 1;
        for (j, b) in h.iter().enumerate() {
            let cost = usize::from(a != b);
            cur[j + 1] = (prev[j] + cost).min(prev[j + 1] + 1).min(cur[j] + 1);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[h.len()] as f64 / r.len() as f64
}

/// The mel front-end, the encoder and the greedy loop, on a real clip.
#[test]
#[ignore = "needs models; run with --include-ignored"]
fn whisper_transcribes_speech_en() {
    let Some(models) = models() else { return };
    let mut engine = Whisper::load(&models.join("whisper-tiny")).unwrap();
    let samples = load_wav(&fixture("speech_en.wav")).unwrap();
    let text = engine.transcribe(&samples, &SttHints { language: Some("en".into()), hotwords: vec![] }).unwrap();
    let error = wer(EXPECTED, &text);
    assert!(error <= 0.2, "WER {error:.3} on {text:?}");
    assert!(text.to_lowercase().contains("quilter"), "{text:?}");
}

/// Without a language hint the model detects one, and the dictionary terms go in
/// as the previous-context prompt.
#[test]
#[ignore = "needs models; run with --include-ignored"]
fn whisper_detects_language_and_accepts_hotwords() {
    let Some(models) = models() else { return };
    let mut engine = Whisper::load(&models.join("whisper-tiny")).unwrap();
    let samples = load_wav(&fixture("speech_en.wav")).unwrap();
    let text = engine
        .transcribe(&samples, &SttHints { language: None, hotwords: vec!["OpenFlow".into()] })
        .unwrap();
    assert!(wer(EXPECTED, &text) <= 0.25, "{text:?}");
}

#[test]
#[ignore = "needs models; run with --include-ignored"]
fn whisper_silence_stays_empty() {
    let Some(models) = models() else { return };
    let mut engine = Whisper::load(&models.join("whisper-tiny")).unwrap();
    let samples = load_wav(&fixture("silence.wav")).unwrap();
    let text = engine.transcribe(&samples, &SttHints { language: Some("en".into()), hotwords: vec![] }).unwrap();
    assert!(text.trim().is_empty(), "silence produced {text:?}");
}

#[test]
#[ignore = "needs models; run with --include-ignored"]
fn unsupported_language_is_an_error() {
    let Some(models) = models() else { return };
    let mut engine = Whisper::load(&models.join("whisper-tiny")).unwrap();
    let samples = load_wav(&fixture("speech_en.wav")).unwrap();
    let err = engine
        .transcribe(&samples, &SttHints { language: Some("xx".into()), hotwords: vec![] })
        .unwrap_err();
    assert!(format!("{err:#}").contains("does not support language"), "{err:#}");
}

//! Engine checks. The model-backed ones are ignored unless run with `--include-ignored`
//! and are skipped when `OPENFLOW_MODELS` (or `<repo>/models`) is absent.

use std::path::{Path, PathBuf};
use std::time::Instant;

use openflow_core::engine::{SAMPLE_RATE, SpeechToText, SttHints, VoiceActivity};
use openflow_engines::audio::load_wav;
use openflow_engines::parakeet::Parakeet;
use openflow_engines::vad::{SileroVad, trim_to_speech};

const EXPECTED: &str =
    "mister quilter is the apostle of the middle classes and we are glad to welcome his gospel";

fn models() -> Option<PathBuf> {
    let dir = std::env::var_os("OPENFLOW_MODELS").map(PathBuf::from).unwrap_or_else(|| {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../models")
    });
    dir.is_dir().then_some(dir)
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name)
}

fn silero_path(models: &Path) -> Option<PathBuf> {
    ["silero-vad/silero_vad.onnx", "silero/model.onnx"]
        .iter()
        .map(|name| models.join(name))
        .find(|path| path.is_file())
}

fn words(text: &str) -> Vec<String> {
    text.split_whitespace()
        .map(|word| word.trim_matches(|c: char| !c.is_alphanumeric() && c != '\'').to_string())
        .filter(|word| !word.is_empty())
        .collect()
}

/// Word-level WER, local so this test does not depend on `openflow_core::distance`.
fn wer(reference: &str, hypothesis: &str) -> f64 {
    let reference = words(&reference.to_lowercase());
    let hypothesis = words(&hypothesis.to_lowercase());
    if reference.is_empty() {
        return if hypothesis.is_empty() { 0.0 } else { 1.0 };
    }
    let mut previous: Vec<usize> = (0..=hypothesis.len()).collect();
    for (i, expected) in reference.iter().enumerate() {
        let mut current = vec![i + 1];
        for (j, actual) in hypothesis.iter().enumerate() {
            let substitute = previous[j] + usize::from(expected != actual);
            current.push((previous[j + 1] + 1).min(current[j] + 1).min(substitute));
        }
        previous = current;
    }
    previous[hypothesis.len()] as f64 / reference.len() as f64
}

#[test]
fn wav_fixtures_decode_to_mono_16k() {
    let speech = load_wav(&fixture("speech_en.wav")).unwrap();
    assert!(
        (speech.len() as i64 - 93680).abs() <= 1,
        "speech_en.wav decoded to {} samples",
        speech.len()
    );
    assert!(speech.iter().all(|s| (-1.0..=1.0).contains(s)));
    assert!(speech.iter().any(|s| s.abs() > 0.01), "expected non-silent audio");

    let stereo = load_wav(&fixture("speech_en_44k_stereo.wav")).unwrap();
    let drift = (stereo.len() as f64 - speech.len() as f64).abs() / speech.len() as f64;
    assert!(drift <= 0.005, "44.1 kHz stereo decoded to {} samples", stereo.len());

    let silence = load_wav(&fixture("silence.wav")).unwrap();
    assert_eq!(silence.len(), 32000);
    assert!(silence.iter().all(|s| *s == 0.0));
}

#[test]
#[ignore = "needs models; run with --include-ignored"]
fn silero_finds_speech_spans_and_none_in_silence() {
    let Some(models) = models() else { return };
    let Some(silero) = silero_path(&models) else { return };

    let speech = load_wav(&fixture("speech_en.wav")).unwrap();
    let mut vad = SileroVad::load(&silero).unwrap();
    let spans = vad.speech_spans(&speech).unwrap();
    let covered: usize = spans.iter().map(|span| span.end - span.start).sum();
    assert!(!spans.is_empty(), "no speech spans found in speech_en.wav");
    assert!(covered >= 4 * SAMPLE_RATE as usize, "speech coverage {covered} samples < 4.0 s");
    assert!(spans.windows(2).all(|pair| pair[0].end <= pair[1].start), "spans overlap");
    assert!(spans.iter().all(|span| span.end <= speech.len() && span.start < span.end));

    let trimmed = trim_to_speech(&speech, &spans);
    assert!(trimmed.len() >= 4 * SAMPLE_RATE as usize);
    assert_eq!(trim_to_speech(&speech, &[]), Vec::<f32>::new());

    let silence = load_wav(&fixture("silence.wav")).unwrap();
    let mut vad = SileroVad::load(&silero).unwrap();
    assert!(vad.speech_spans(&silence).unwrap().is_empty(), "silence produced spans");
}

#[test]
#[ignore = "needs models; run with --include-ignored"]
fn parakeet_transcribes_speech_en() {
    let Some(models) = models() else { return };
    let samples = load_wav(&fixture("speech_en.wav")).unwrap();
    let mut parakeet = Parakeet::load(&models.join("parakeet-tdt-0.6b-v3")).unwrap();

    let started = Instant::now();
    let text = parakeet.transcribe(&samples, &SttHints::default()).unwrap();
    let elapsed_ms = started.elapsed().as_millis();
    println!("parakeet transcript ({elapsed_ms} ms): {text:?}");

    let normalize = |s: &str| s.to_lowercase().replace("mr.", "mister");
    let error = wer(&normalize(EXPECTED), &normalize(&text));
    println!("wer = {error}");
    assert!(error <= 0.15, "wer {error} > 0.15 for {text:?}");
}

#[test]
#[ignore = "needs models; run with --include-ignored"]
fn parakeet_silence_is_empty() {
    let Some(models) = models() else { return };
    let samples = load_wav(&fixture("silence.wav")).unwrap();
    let mut parakeet = Parakeet::load(&models.join("parakeet-tdt-0.6b-v3")).unwrap();
    let text = parakeet.transcribe(&samples, &SttHints::default()).unwrap();
    println!("parakeet silence transcript: {text:?}");
    assert!(text.trim().is_empty(), "silence produced {text:?}");
}

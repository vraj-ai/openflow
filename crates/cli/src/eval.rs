//! `openflow eval`: ASR word error rate on recorded clips, and cleanup quality
//! (WER against a hand-written expected text) with and without the LLM.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::Context;
use openflow_core::controller::clean;
use openflow_core::desktop::AppInfo;
use openflow_core::distance::{normalize_words, wer};
use openflow_core::engine::SttHints;
use openflow_core::settings::Settings;
use openflow_engines::{audio, stack, vad};
use serde::{Deserialize, Serialize};

#[derive(clap::Args)]
pub struct EvalArgs {
    /// JSONL of `{"audio": "<file in eval/audio>", "text": "<reference>"}`.
    #[arg(long)]
    asr: Option<PathBuf>,
    /// JSONL of `{"raw": "...", "expected": "...", "tags": [...], "app": "..."}`.
    #[arg(long)]
    cleanup: Option<PathBuf>,
    /// Evaluate cleanup with rules only.
    #[arg(long)]
    no_llm: bool,
    #[arg(long)]
    limit: Option<usize>,
    /// Write per-case results here as JSONL.
    #[arg(long)]
    out: Option<PathBuf>,
}

#[derive(Deserialize)]
struct AsrCase {
    audio: String,
    text: String,
}

#[derive(Deserialize)]
struct CleanupCase {
    raw: String,
    expected: String,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    app: Option<String>,
}

#[derive(Serialize)]
struct CaseResult<'a> {
    kind: &'a str,
    input: &'a str,
    expected: &'a str,
    output: String,
    wer: f64,
    exact: bool,
    used_llm: bool,
    rejected: Option<String>,
    ms: u128,
    tags: &'a [String],
}

fn read_jsonl<T: for<'de> Deserialize<'de>>(path: &Path, limit: Option<usize>) -> anyhow::Result<Vec<T>> {
    let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .take(limit.unwrap_or(usize::MAX))
        .enumerate()
        .map(|(i, l)| serde_json::from_str(l).with_context(|| format!("{}:{}", path.display(), i + 1)))
        .collect()
}

/// LibriSpeech references spell out "mister"; models print "Mr.".
fn asr_normalize(text: &str) -> String {
    normalize_words(text)
        .into_iter()
        .map(|w| match w.as_str() {
            "mr" | "mr." => "mister".to_string(),
            "mrs" | "mrs." => "missus".to_string(),
            "dr" | "dr." => "doctor".to_string(),
            _ => w,
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn percentile(sorted: &[Duration], p: f64) -> u128 {
    if sorted.is_empty() {
        return 0;
    }
    sorted[((sorted.len() - 1) as f64 * p).round() as usize].as_millis()
}

pub fn run(args: EvalArgs, settings: &Settings, root: &Path) -> anyhow::Result<()> {
    let mut out = match &args.out {
        Some(p) => Some(std::io::BufWriter::new(std::fs::File::create(p)?)),
        None => None,
    };
    let mut write = |r: &CaseResult| -> anyhow::Result<()> {
        if let Some(f) = out.as_mut() {
            use std::io::Write;
            writeln!(f, "{}", serde_json::to_string(r)?)?;
        }
        Ok(())
    };

    if let Some(path) = &args.asr {
        let cases: Vec<AsrCase> = read_jsonl(path, args.limit)?;
        let dir = path.parent().unwrap_or(Path::new(".")).join("audio");
        let mut engines = stack::load(settings, root, stack::Load { vad: true, llm: false, multilingual: false })?;
        let (mut errors, mut words, mut audio_secs) = (0.0, 0usize, 0.0);
        let mut times = vec![];
        for case in &cases {
            let samples = audio::load_wav(&dir.join(&case.audio))?;
            audio_secs += samples.len() as f64 / 16000.0;
            let started = Instant::now();
            let samples = match engines.vad.as_mut() {
                Some(v) => vad::trim_to_speech(&samples, &v.speech_spans(&samples)?),
                None => samples,
            };
            let text = engines.stt.transcribe(&samples, &SttHints::default())?;
            let elapsed = started.elapsed();
            times.push(elapsed);
            let reference = asr_normalize(&case.text);
            let n = reference.split_whitespace().count();
            let w = wer(&reference, &asr_normalize(&text));
            errors += w * n as f64;
            words += n;
            write(&CaseResult {
                kind: "asr",
                input: &case.audio,
                expected: &case.text,
                exact: w == 0.0,
                output: text,
                wer: w,
                used_llm: false,
                rejected: None,
                ms: elapsed.as_millis(),
                tags: &[],
            })?;
        }
        times.sort();
        let total: Duration = times.iter().sum();
        println!(
            "ASR  {} clips, {:.0} s audio: WER {:.2}% | latency p50 {} ms, p95 {} ms | {:.1}x realtime",
            cases.len(),
            audio_secs,
            errors / words.max(1) as f64 * 100.0,
            percentile(&times, 0.5),
            percentile(&times, 0.95),
            audio_secs / total.as_secs_f64().max(1e-9),
        );
    }

    if let Some(path) = &args.cleanup {
        let cases: Vec<CleanupCase> = read_jsonl(path, args.limit)?;
        let mut s = settings.clone();
        s.cleanup.llm_enabled &= !args.no_llm;
        let mut llm = crate::load_llm(&s, root)?;
        let (mut wer_sum, mut raw_wer_sum, mut exact, mut llm_used, mut rejected) = (0.0, 0.0, 0, 0, 0);
        let mut times = vec![];
        let mut by_tag: std::collections::BTreeMap<String, (f64, usize)> = Default::default();
        for case in &cases {
            let style = s.styles.resolve(&AppInfo { id: case.app.clone(), name: None, url: None });
            let started = Instant::now();
            let cleaned = clean(&case.raw, &s, &style, llm.as_deref_mut().map(|l| l as _));
            let elapsed = started.elapsed();
            times.push(elapsed);
            let w = wer(&case.expected, &cleaned.text);
            let is_exact = cleaned.text.trim() == case.expected.trim();
            wer_sum += w;
            raw_wer_sum += wer(&case.expected, &case.raw);
            exact += is_exact as usize;
            llm_used += cleaned.used_llm as usize;
            rejected += cleaned.rejected.is_some() as usize;
            for tag in &case.tags {
                let e = by_tag.entry(tag.clone()).or_default();
                e.0 += w;
                e.1 += 1;
            }
            write(&CaseResult {
                kind: "cleanup",
                input: &case.raw,
                expected: &case.expected,
                output: cleaned.text,
                wer: w,
                exact: is_exact,
                used_llm: cleaned.used_llm,
                rejected: cleaned.rejected.map(|r| format!("{r:?}")),
                ms: elapsed.as_millis(),
                tags: &case.tags,
            })?;
        }
        times.sort();
        let n = cases.len().max(1) as f64;
        let mode = if llm.is_some() { s.cleanup.llm_model.as_str() } else { "rules only" };
        println!(
            "CLEANUP ({mode}) {} cases: WER vs expected {:.2}% (raw transcript {:.2}%) | exact {}/{} | LLM used {} rejected {} | latency p50 {} ms, p95 {} ms",
            cases.len(),
            wer_sum / n * 100.0,
            raw_wer_sum / n * 100.0,
            exact,
            cases.len(),
            llm_used,
            rejected,
            percentile(&times, 0.5),
            percentile(&times, 0.95),
        );
        for (tag, (sum, count)) in by_tag {
            println!("  {tag:<14} {count:>3} cases  WER {:.2}%", sum / count as f64 * 100.0);
        }
    }
    Ok(())
}

//! Real-model tests for the local quantized LLM. Skipped when the 1.7B Qwen3 GGUF is absent.

use std::path::PathBuf;

use openflow_core::engine::{ChatPrompt, GenOptions, TextGenerator};
use openflow_engines::llm::{Family, LocalLlm};

const REPEAT_SYSTEM: &str = "You repeat the user's text exactly, with no other words.";
const CLEANUP_SYSTEM: &str = "Clean up this dictated text: remove filler words like um and uh, fix punctuation. Output only the cleaned text.";

fn model_dir() -> Option<PathBuf> {
    let base = std::env::var("OPENFLOW_MODELS")
        .unwrap_or_else(|_| format!("{}/../../models", env!("CARGO_MANIFEST_DIR")));
    let dir = PathBuf::from(base).join("qwen3-1.7b");
    let present = dir.join("model.gguf").exists() && dir.join("tokenizer.json").exists();
    present.then_some(dir)
}

macro_rules! load_or_skip {
    () => {
        match model_dir() {
            Some(dir) => LocalLlm::load(&dir.join("model.gguf"), &dir.join("tokenizer.json"))
                .expect("load qwen3-1.7b"),
            None => {
                eprintln!("skipping: OPENFLOW_MODELS qwen3-1.7b not present");
                return;
            }
        }
    };
}

#[test]
#[ignore = "needs models"]
fn family_is_qwen3_and_render_opens_assistant_turn() {
    let llm = load_or_skip!();

    assert_eq!(llm.family(), Family::Qwen3);
    let rendered = llm.render(&ChatPrompt {
        system: "be terse".into(),
        user: "hello".into(),
    });
    assert!(rendered.contains("<|im_start|>assistant"), "rendered: {rendered}");
}

#[test]
#[ignore = "needs models"]
fn repeats_input_and_cleans_dictation() {
    let mut llm = load_or_skip!();

    let echo = llm
        .generate(
            &ChatPrompt {
                system: REPEAT_SYSTEM.into(),
                user: "The quick brown fox.".into(),
            },
            &GenOptions {
                max_tokens: 20,
                temperature: 0.0,
            },
        )
        .expect("generate echo");
    println!("echo output: {echo:?}");
    assert!(echo.contains("quick brown fox"), "echo output: {echo:?}");

    let cleaned = llm
        .generate(
            &ChatPrompt {
                system: CLEANUP_SYSTEM.into(),
                user: "um so i think we should uh meet on tuesday".into(),
            },
            &GenOptions {
                max_tokens: 64,
                temperature: 0.0,
            },
        )
        .expect("generate cleanup");
    println!("cleanup output: {cleaned:?}");

    let lower = cleaned.to_lowercase();
    assert!(lower.contains("tuesday"), "cleanup output: {cleaned:?}");
    assert!(!lower.contains("um "), "cleanup output: {cleaned:?}");
    assert!(!lower.contains("uh "), "cleanup output: {cleaned:?}");
    assert!(
        cleaned.split_whitespace().count() < 25,
        "cleanup output: {cleaned:?}"
    );
}

#[test]
#[ignore = "needs models"]
fn cached_system_prefix_matches_cold_output_and_is_faster() {
    let mut llm = load_or_skip!();
    let system = format!("{CLEANUP_SYSTEM} {}", "Keep the speaker's words. ".repeat(40));
    let opts = GenOptions { max_tokens: 32, temperature: 0.0 };
    let ask = |user: &str| ChatPrompt { system: system.clone(), user: user.into() };

    let t0 = std::time::Instant::now();
    let cold = llm.generate(&ask("um so i think we should uh meet on tuesday"), &opts).unwrap();
    let cold_ms = t0.elapsed().as_millis();
    let t1 = std::time::Instant::now();
    let warm = llm.generate(&ask("um so i think we should uh meet on tuesday"), &opts).unwrap();
    let warm_ms = t1.elapsed().as_millis();
    let other = llm.generate(&ask("uh can you send the report by friday"), &opts).unwrap();
    println!("cold {cold_ms} ms, warm {warm_ms} ms: {cold:?} / {other:?}");
    assert_eq!(cold, warm);
    assert!(other.to_lowercase().contains("friday"), "{other:?}");
    assert!(warm_ms < cold_ms, "prefix cache did not help: cold {cold_ms} ms, warm {warm_ms} ms");
}

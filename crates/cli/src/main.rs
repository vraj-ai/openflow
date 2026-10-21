//! `openflow`: the headless surface. Every feature of the app can be driven
//! from here, which is how it is verified on machines without a display.

mod eval;

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, mpsc};
use std::time::Instant;

use anyhow::Context;
use clap::{Parser, Subcommand, ValueEnum};
use openflow_core::catalog;
use openflow_core::controller::{Controller, clean};
use openflow_core::desktop::{AppInfo, FieldSnapshot};
use openflow_core::dictionary::{DictEntry, EntrySource};
use openflow_core::history::History;
use openflow_core::hotkey::{Machine, Mode};
use openflow_core::mock::MockDesktop;
use openflow_core::runtime::{self, AudioSource, Status};
use openflow_core::settings::Settings;
use openflow_core::snippets::Snippet;
use openflow_engines::{audio, download, stack};
use openflow_platform::paths;

#[derive(Parser)]
#[command(name = "openflow", version, about = "Local voice dictation with LLM cleanup")]
struct Cli {
    /// Settings file (default: the app's settings.json).
    #[arg(long, global = true)]
    settings: Option<PathBuf>,
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Clone, Copy, ValueEnum)]
enum ModeArg {
    Dictation,
    Command,
}

#[derive(Subcommand)]
enum Cmd {
    /// List or download models.
    Models {
        #[command(subcommand)]
        action: ModelsCmd,
    },
    /// Speech-to-text only.
    Transcribe {
        wav: PathBuf,
        #[arg(long)]
        no_vad: bool,
    },
    /// Run the cleanup pipeline on text.
    Clean {
        text: String,
        /// App id used to pick the style (e.g. com.apple.mail).
        #[arg(long)]
        app: Option<String>,
        #[arg(long)]
        no_llm: bool,
    },
    /// Run the full pipeline on a WAV against a simulated desktop and print
    /// what would be pasted.
    Dictate {
        wav: PathBuf,
        #[arg(long, value_enum, default_value = "dictation")]
        mode: ModeArg,
        #[arg(long)]
        app: Option<String>,
        /// Simulated selection for Command Mode.
        #[arg(long)]
        selection: Option<String>,
        /// Simulated focused text field contents.
        #[arg(long)]
        field: Option<String>,
        /// Watch the simulated field and report the dictionary entries the
        /// correction would teach (the same watch the daemon runs after a paste).
        #[arg(long)]
        learn: bool,
        #[arg(long)]
        no_llm: bool,
    },
    /// List, add or remove personal dictionary terms.
    Dict {
        #[command(subcommand)]
        action: DictCmd,
    },
    /// List, add or remove snippets.
    Snippet {
        #[command(subcommand)]
        action: SnippetCmd,
    },
    /// Read or write one setting with a dotted key (see `openflow settings`).
    Set { key: String, value: String },
    /// Measure ASR WER and cleanup quality on the eval sets.
    Eval(eval::EvalArgs),
    /// Listen for the global hotkey, record the microphone, paste into the focused app.
    Daemon,
    /// Print the settings file path and contents.
    Settings,
    /// Check this machine: models, accelerator backends, native library, permissions.
    Doctor,
    /// Show, clear or export the dictation history.
    History {
        /// Show only the newest N entries.
        #[arg(long)]
        limit: Option<usize>,
        /// Empty the history file.
        #[arg(long)]
        clear: bool,
        /// Print the stored JSONL instead of a table.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum ModelsCmd {
    List,
    Download { ids: Vec<String> },
    /// Delete a downloaded model. Prints what would go unless --yes is given.
    Remove {
        ids: Vec<String>,
        #[arg(long)]
        yes: bool,
    },
}

#[derive(Subcommand)]
enum DictCmd {
    List,
    Add {
        term: String,
        /// A phrase the speech model writes instead of the term (repeatable).
        #[arg(long = "sounds-like")]
        sounds_like: Vec<String>,
    },
    Remove { term: String },
}

#[derive(Subcommand)]
enum SnippetCmd {
    List,
    Add { cue: String, text: String },
    Remove { cue: String },
}

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let cli = Cli::parse();
    let settings_path = cli.settings.clone().unwrap_or_else(paths::settings_file);
    let settings = Settings::load(&settings_path).with_context(|| format!("reading {}", settings_path.display()))?;
    let root = stack::models_root(&paths::data_dir());
    match cli.command {
        Cmd::Models { action: ModelsCmd::List } => {
            for m in catalog::all() {
                let state = if m.is_installed(&root) { "installed" } else { "-" };
                println!("{:<26} {:<13} {:>6} MB  {:<9}  {}", m.id, format!("{:?}", m.kind), m.download_size() / 1_000_000, state, m.description);
            }
            println!("models dir: {}", root.display());
        }
        Cmd::Models { action: ModelsCmd::Download { ids } } => {
            for id in ids {
                let spec = catalog::get(&id).with_context(|| format!("unknown model {id}"))?;
                let mut last = 0;
                download::download(
                    spec,
                    &root,
                    &mut |p| {
                        let pct = p.done * 100 / p.total.max(1);
                        if pct != last {
                            last = pct;
                            eprint!("\r{id}: {pct}%");
                        }
                    },
                    &|| false,
                )?;
                eprintln!("\r{id}: installed in {}", spec.dir(&root).display());
            }
        }
        Cmd::Models { action: ModelsCmd::Remove { ids, yes } } => {
            for id in ids {
                let spec = catalog::get(&id).with_context(|| format!("unknown model {id}"))?;
                let dir = spec.dir(&root);
                if !dir.exists() {
                    eprintln!("{id}: not downloaded");
                    continue;
                }
                if yes {
                    std::fs::remove_dir_all(&dir)?;
                    eprintln!("{id}: deleted {}", dir.display());
                } else {
                    eprintln!("{id}: would delete {} ({} MB); pass --yes", dir.display(), spec.download_size() / 1_000_000);
                }
            }
        }
        Cmd::Transcribe { wav, no_vad } => {
            let mut s = settings.clone();
            s.stt.vad = !no_vad;
            let mut engines = stack::load(&s, &root, stack::Load { vad: !no_vad, llm: false, multilingual: true })?;
            let samples = audio::load_wav(&wav)?;
            let started = Instant::now();
            let samples = match engines.vad.as_mut() {
                Some(vad) => {
                    let spans = vad.speech_spans(&samples)?;
                    openflow_engines::vad::trim_to_speech(&samples, &spans)
                }
                None => samples,
            };
            let hints = openflow_core::engine::SttHints {
                language: s.stt.language.clone(),
                hotwords: s.dictionary.hotwords(),
            };
            let text = if openflow_core::controller::needs_multilingual(s.stt.language.as_deref()) {
                match engines.multilingual.as_mut() {
                    Some(m) => m.transcribe(&samples, &hints)?,
                    None => engines.stt.transcribe(&samples, &hints)?,
                }
            } else {
                engines.stt.transcribe(&samples, &hints)?
            };
            eprintln!("({} ms)", started.elapsed().as_millis());
            println!("{text}");
        }
        Cmd::Clean { text, app, no_llm } => {
            let mut s = settings.clone();
            s.cleanup.llm_enabled &= !no_llm;
            let mut llm = load_llm(&s, &root)?;
            let style = s.styles.resolve(&AppInfo { id: app, name: None, url: None });
            let started = Instant::now();
            let out = clean(&text, &s, &style, llm.as_deref_mut().map(|l| l as _));
            eprintln!("({} ms, llm: {}, rejected: {:?})", started.elapsed().as_millis(), out.used_llm, out.rejected);
            println!("{}", out.text);
        }
        Cmd::Dictate { wav, mode, app, selection, field, learn, no_llm } => {
            let mut s = settings.clone();
            s.cleanup.llm_enabled &= !no_llm || matches!(mode, ModeArg::Command);
            let engines = stack::load(&s, &root, stack::Load { vad: true, llm: s.cleanup.llm_enabled, multilingual: true })?;
            let desktop = MockDesktop::default();
            desktop.state().app = AppInfo { id: app, name: None, url: None };
            desktop.state().ax_selection = selection;
            desktop.state().field = field.map(|value| FieldSnapshot { value, selection: None });
            let mut controller = Controller::new(s, engines, desktop.clone(), None);
            let samples = audio::load_wav(&wav)?;
            let mode = match mode {
                ModeArg::Dictation => Mode::Dictation,
                ModeArg::Command => Mode::Command,
            };
            let report = controller.process(mode, &samples)?;
            eprintln!(
                "raw: {}\nstt: {} ms, cleanup: {} ms, llm: {}, rejected: {:?}",
                report.raw,
                report.stt.as_millis(),
                report.cleanup.as_millis(),
                report.used_llm,
                report.rejected
            );
            let mut learned = Vec::new();
            if learn && mode == Mode::Dictation {
                learned = controller.learn_after_insert(
                    &std::sync::atomic::AtomicBool::new(false),
                    &mut std::thread::sleep,
                );
                for entry in &learned {
                    eprintln!("learned: {} (sounds like {:?})", entry.term, entry.sounds_like);
                }
            }
            println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                "outcome": format!("{:?}", report.outcome),
                "pasted": desktop.pasted(),
                "keys": format!("{:?}", desktop.state().keys),
                "notifications": desktop.state().notifications,
                "learned": learned.iter().map(|e| &e.term).collect::<Vec<_>>(),
            }))?);
        }
        Cmd::Dict { action } => {
            let mut s = settings.clone();
            match action {
                DictCmd::List => {
                    for e in &s.dictionary.entries {
                        println!(
                            "{:<28} {:?} {}",
                            e.term,
                            e.source,
                            e.sounds_like.join(" | ")
                        );
                    }
                    println!("{} terms", s.dictionary.entries.len());
                }
                DictCmd::Add { term, sounds_like } => {
                    s.dictionary.upsert(DictEntry {
                        term: term.clone(),
                        sounds_like,
                        source: EntrySource::Manual,
                    });
                    s.save(&settings_path)?;
                    println!("{term}: saved to {}", settings_path.display());
                }
                DictCmd::Remove { term } => {
                    let before = s.dictionary.entries.len();
                    s.dictionary.entries.retain(|e| !e.term.eq_ignore_ascii_case(&term));
                    s.save(&settings_path)?;
                    println!("{term}: removed {} entry(s)", before - s.dictionary.entries.len());
                }
            }
        }
        Cmd::Snippet { action } => {
            let mut s = settings.clone();
            match action {
                SnippetCmd::List => {
                    for x in &s.snippets.items {
                        println!("{:<28} {}", x.cue, x.text.replace('\n', " \\n "));
                    }
                    println!("{} snippets", s.snippets.items.len());
                }
                SnippetCmd::Add { cue, text } => {
                    s.snippets.items.retain(|x| x.cue.to_lowercase() != cue.to_lowercase());
                    s.snippets.items.push(Snippet { cue: cue.clone(), text });
                    s.save(&settings_path)?;
                    println!("{cue}: saved to {}", settings_path.display());
                }
                SnippetCmd::Remove { cue } => {
                    let before = s.snippets.items.len();
                    s.snippets.items.retain(|x| x.cue.to_lowercase() != cue.to_lowercase());
                    s.save(&settings_path)?;
                    println!("{cue}: removed {} snippet(s)", before - s.snippets.items.len());
                }
            }
        }
        Cmd::Set { key, value } => {
            let mut s = settings.clone();
            apply_setting(&mut s, &key, &value)?;
            s.save(&settings_path)?;
            println!("{key} = {value}\n{}", settings_path.display());
        }
        Cmd::Eval(args) => eval::run(args, &settings, &root)?,
        Cmd::Daemon => daemon(settings, &root, &settings_path)?,
        Cmd::Doctor => {
            let perms = openflow_platform::permissions(false);
            let backends = openflow_engines::backends();
            println!("settings:   {}", settings_path.display());
            println!("models dir: {}", root.display());
            println!(
                "cuda:       built {}, available {}",
                backends.cuda_built, backends.cuda_available
            );
            println!(
                "metal:      built {}, available {}",
                backends.metal_built, backends.metal_available
            );
            println!("coreml:     built {}", backends.coreml_built);
            match std::env::var_os("ORT_DYLIB_PATH") {
                Some(p) => match openflow_engines::ort_env::init() {
                    Ok(()) => println!("onnxruntime: {} (loads)", p.to_string_lossy()),
                    Err(err) => {
                        let reason = format!("{err:#}").replace('\n', " ");
                        println!("onnxruntime: {} (failed: {reason})", p.to_string_lossy());
                    }
                },
                None => println!("onnxruntime: ORT_DYLIB_PATH is not set"),
            }
            println!("accessibility: {}, input monitoring: {}", perms.accessibility, perms.input_monitoring);
            let mut missing = 0;
            for m in catalog::all() {
                let state = if m.is_installed(&root) { "ok" } else { missing += 1; "missing" };
                println!("  {:<26} {state}", m.id);
            }
            if missing > 0 {
                println!("{missing} model(s) missing: openflow models download <id>");
            }
            let used = [
                &settings.stt.model,
                &settings.stt.multilingual_model,
                &settings.cleanup.llm_model,
            ];
            for id in used {
                if !catalog::get(id).is_some_and(|m| m.is_installed(&root)) {
                    println!("settings point at a model that is not downloaded: {id}");
                }
            }
        }
        Cmd::History { limit, clear, json } => {
            let history = History::new(paths::history_file(), settings.history.max_entries);
            if clear {
                history.clear()?;
                println!("history cleared");
            }
            let entries = history.load()?;
            let skip = entries.len().saturating_sub(limit.unwrap_or(usize::MAX));
            for e in &entries[skip..] {
                if json {
                    println!("{}", serde_json::to_string(e)?);
                } else {
                    println!(
                        "{} {:<9} {:<24} {}{}",
                        openflow_core::history::format_time(e.at_ms),
                        format!("{:?}", e.mode).to_lowercase(),
                        e.app.id.as_deref().unwrap_or("-"),
                        e.text.replace('\n', " / "),
                        if e.rejected.is_some() { "  [llm rejected]" } else { "" }
                    );
                }
            }
            if !json {
                println!("{} entries ({})", entries.len(), paths::history_file().display());
            }
        }
        Cmd::Settings => {
            println!("{}", settings_path.display());
            println!("{}", serde_json::to_string_pretty(&settings)?);
        }
    }
    Ok(())
}

pub(crate) fn load_llm(
    settings: &Settings,
    root: &Path,
) -> anyhow::Result<Option<Box<dyn openflow_core::engine::TextGenerator>>> {
    if !settings.cleanup.llm_enabled {
        return Ok(None);
    }
    let (_, dir) = stack::installed(&settings.cleanup.llm_model, root)?;
    Ok(Some(Box::new(openflow_engines::llm::LocalLlm::load(&dir.join("model.gguf"), &dir.join("tokenizer.json"))?)))
}

struct Mic(openflow_platform::capture::Recorder);

impl AudioSource for Mic {
    fn start(&mut self) {
        self.0.start();
    }
    fn stop(&mut self) -> Vec<f32> {
        let rec = self.0.stop();
        let mono = audio::to_mono(&rec.samples, rec.channels);
        audio::resample(&mono, rec.sample_rate, openflow_core::engine::SAMPLE_RATE)
    }
}

/// Every key `openflow set` accepts, in one place, so a typo cannot write a
/// field that nothing reads. Model ids are checked against the catalog.
const SETTABLE: &[&str] = &[
    "cleanup.llm_enabled",
    "cleanup.llm_model",
    "cleanup.min_words_for_llm",
    "cleanup.rules.fillers",
    "cleanup.rules.self_corrections",
    "cleanup.rules.lists",
    "cleanup.rules.spoken_commands",
    "stt.model",
    "stt.multilingual_model",
    "stt.language",
    "stt.vad",
    "insertion.restore_clipboard",
    "insertion.restore_delay_ms",
    "insertion.press_enter_command",
    "history.enabled",
    "history.max_entries",
    "learning.auto_learn",
    "learning.watch_ms",
    "styles.default_tone",
];

fn apply_setting(s: &mut Settings, key: &str, value: &str) -> anyhow::Result<()> {
    let on = |v: &str| -> anyhow::Result<bool> {
        match v.to_ascii_lowercase().as_str() {
            "true" | "yes" | "on" | "1" => Ok(true),
            "false" | "no" | "off" | "0" => Ok(false),
            other => anyhow::bail!("expected on/off, got {other:?}"),
        }
    };
    let model = |v: &str, expected: openflow_core::catalog::ModelKind| -> anyhow::Result<String> {
        let spec = catalog::get(v).with_context(|| format!("unknown model {v}; see `openflow models list`"))?;
        anyhow::ensure!(spec.kind == expected, "{v} is a {:?} model, not {:?}", spec.kind, expected);
        Ok(v.to_string())
    };
    match key {
        "cleanup.llm_enabled" => s.cleanup.llm_enabled = on(value)?,
        "cleanup.llm_model" => s.cleanup.llm_model = model(value, catalog::ModelKind::Llm)?,
        "cleanup.min_words_for_llm" => s.cleanup.min_words_for_llm = value.parse()?,
        "cleanup.rules.fillers" => s.cleanup.rules.fillers = on(value)?,
        "cleanup.rules.self_corrections" => s.cleanup.rules.self_corrections = on(value)?,
        "cleanup.rules.lists" => s.cleanup.rules.lists = on(value)?,
        "cleanup.rules.spoken_commands" => s.cleanup.rules.spoken_commands = on(value)?,
        "stt.model" => s.stt.model = model(value, catalog::ModelKind::Stt)?,
        "stt.multilingual_model" => {
            s.stt.multilingual_model = model(value, catalog::ModelKind::Multilingual)?
        }
        "stt.language" => {
            s.stt.language = (!matches!(value, "none" | "auto" | "")).then(|| value.to_string())
        }
        "stt.vad" => s.stt.vad = on(value)?,
        "insertion.restore_clipboard" => s.insertion.restore_clipboard = on(value)?,
        "insertion.restore_delay_ms" => s.insertion.restore_delay_ms = value.parse()?,
        "insertion.press_enter_command" => s.insertion.press_enter_command = on(value)?,
        "history.enabled" => s.history.enabled = on(value)?,
        "history.max_entries" => s.history.max_entries = value.parse()?,
        "learning.auto_learn" => s.learning.auto_learn = on(value)?,
        "learning.watch_ms" => s.learning.watch_ms = value.parse()?,
        "styles.default_tone" => {
            s.styles.default_tone = serde_json::from_str(&format!("\"{value}\""))
                .context("expected formal, casual, very_casual or excited")?
        }
        other => anyhow::bail!("unknown key {other}; settable keys: {}", SETTABLE.join(", ")),
    }
    Ok(())
}

fn daemon(settings: Settings, root: &Path, settings_path: &Path) -> anyhow::Result<()> {
    let perms = openflow_platform::permissions(true);
    if !perms.accessibility || !perms.input_monitoring {
        eprintln!("Grant Accessibility and Input Monitoring to this terminal in System Settings > Privacy & Security, then restart.");
    }
    let engines = stack::load(&settings, root, stack::Load { vad: true, llm: true, multilingual: true })?;
    let history = settings
        .history
        .enabled
        .then(|| History::new(paths::history_file(), settings.history.max_entries));
    let mut machine = Machine::new(&settings.hotkeys);
    let (tx, rx) = mpsc::channel();
    let recording = Arc::new(AtomicBool::new(false));
    openflow_platform::spawn_hotkey_listener(&settings.hotkeys, tx, recording.clone())?;
    let mut mic = Mic(openflow_platform::capture::Recorder::open_default()?);
    eprintln!("OpenFlow is listening. Hold {:?} to dictate, {:?} for Command Mode.", settings.hotkeys.dictate, settings.hotkeys.command);
    let mut controller = Controller::new(settings, engines, openflow_platform::SystemDesktop::new(), history);
    runtime::run(
        rx,
        &mut machine,
        &mut mic,
        &mut controller,
        recording,
        &mut |status| match status {
            Status::Recording(mode) => eprintln!("recording ({mode:?})"),
            Status::Processing(_) => eprintln!("processing"),
            Status::Cancelled => eprintln!("cancelled"),
            Status::Done(r) => eprintln!("{:?}  [stt {} ms, cleanup {} ms]", r.outcome, r.stt.as_millis(), r.cleanup.as_millis()),
            Status::Failed(e) => eprintln!("error: {e}"),
        },
        &mut |learned| {
            // Write straight to the settings file: a daemon can run for days, and
            // the controller has already learned these entries in memory.
            let save = Settings::load(settings_path).and_then(|mut s| {
                for entry in learned {
                    s.dictionary.upsert(entry.clone());
                }
                s.save(settings_path)
            });
            match save {
                Ok(()) => eprintln!(
                    "learned: {}",
                    learned.iter().map(|e| e.term.as_str()).collect::<Vec<_>>().join(", ")
                ),
                Err(e) => eprintln!("could not save the learned words: {e:#}"),
            }
        },
        &mut std::thread::sleep,
    );
    Ok(())
}

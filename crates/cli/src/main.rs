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
use openflow_core::history::History;
use openflow_core::hotkey::{Machine, Mode};
use openflow_core::mock::MockDesktop;
use openflow_core::runtime::{self, AudioSource, Status};
use openflow_core::settings::Settings;
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
        #[arg(long)]
        no_llm: bool,
    },
    /// Measure ASR WER and cleanup quality on the eval sets.
    Eval(eval::EvalArgs),
    /// Listen for the global hotkey, record the microphone, paste into the focused app.
    Daemon,
    /// Print the settings file path and contents.
    Settings,
}

#[derive(Subcommand)]
enum ModelsCmd {
    List,
    Download { ids: Vec<String> },
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
            let text = engines.stt.transcribe(&samples, &hints)?;
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
        Cmd::Dictate { wav, mode, app, selection, field, no_llm } => {
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
            println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                "outcome": format!("{:?}", report.outcome),
                "pasted": desktop.pasted(),
                "keys": format!("{:?}", desktop.state().keys),
                "notifications": desktop.state().notifications,
            }))?);
        }
        Cmd::Eval(args) => eval::run(args, &settings, &root)?,
        Cmd::Daemon => daemon(settings, &root)?,
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

fn daemon(settings: Settings, root: &Path) -> anyhow::Result<()> {
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
    runtime::run(rx, &mut machine, &mut mic, &mut controller, recording, &mut |status| match status {
        Status::Recording(mode) => eprintln!("recording ({mode:?})"),
        Status::Processing(_) => eprintln!("processing"),
        Status::Cancelled => eprintln!("cancelled"),
        Status::Done(r) => eprintln!("{:?}  [stt {} ms, cleanup {} ms]", r.outcome, r.stt.as_millis(), r.cleanup.as_millis()),
        Status::Failed(e) => eprintln!("error: {e}"),
    });
    Ok(())
}

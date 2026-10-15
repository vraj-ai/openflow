//! Turns a finished recording into text in the user's app: speech-to-text,
//! cleanup, insertion, Command Mode, and history.

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::desktop::{self, AppInfo, Desktop, Keystroke, PasteTiming};
use crate::distance::normalize_words;
use crate::engine::{GenOptions, SpeechToText, SttHints, TextGenerator, VoiceActivity};
use crate::guard::{self, Rejection};
use crate::history::{History, HistoryEntry};
use crate::hotkey::Mode;
use crate::prompt::{self, CommandTarget};
use crate::settings::Settings;
use crate::style::Style;
use crate::text::apply_rules;

/// Languages Parakeet TDT v3 transcribes. Anything else routes to the
/// multilingual (Whisper) engine.
pub const PARAKEET_LANGUAGES: &[&str] = &[
    "bg", "hr", "cs", "da", "nl", "en", "et", "fi", "fr", "de", "el", "hu", "it", "lv", "lt", "mt", "pl", "pt",
    "ro", "sk", "sl", "es", "sv", "ru", "uk",
];

pub struct Engines {
    pub stt: Box<dyn SpeechToText>,
    pub multilingual: Option<Box<dyn SpeechToText>>,
    pub vad: Option<Box<dyn VoiceActivity>>,
    pub llm: Option<Box<dyn TextGenerator>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Cleaned {
    pub text: String,
    pub used_llm: bool,
    pub rejected: Option<Rejection>,
    pub press_enter: bool,
}

/// Strips a trailing spoken "press enter" and reports whether it was there.
pub fn split_press_enter(text: &str) -> (String, bool) {
    let words = normalize_words(text);
    if words.len() >= 2 && words[words.len() - 2..] == ["press", "enter"] {
        let lower = text.to_lowercase();
        if let Some(at) = lower.rfind("press") {
            let head = text[..at].trim_end().trim_end_matches(',').trim_end();
            return (head.to_string(), true);
        }
    }
    (text.to_string(), false)
}

/// The dictation cleanup pipeline. Pure apart from the optional LLM call.
pub fn clean(raw: &str, settings: &Settings, style: &Style, llm: Option<&mut dyn TextGenerator>) -> Cleaned {
    let (raw, press_enter) = if settings.insertion.press_enter_command {
        split_press_enter(raw)
    } else {
        (raw.to_string(), false)
    };
    let rules = apply_rules(&raw, &settings.cleanup.rules);
    let dict = &settings.dictionary;
    let base = dict.apply(&rules);
    if let Some(snippet) = settings.snippets.whole_match(&base) {
        return Cleaned { text: snippet.text.clone(), used_llm: false, rejected: None, press_enter };
    }

    let mut used_llm = false;
    let mut rejected = None;
    let mut text = base.clone();
    let long_enough = base.split_whitespace().count() >= settings.cleanup.min_words_for_llm;
    if let (true, true, Some(llm)) = (settings.cleanup.llm_enabled, long_enough, llm) {
        let glossary = dict.glossary();
        let prompt = prompt::cleanup_prompt(&base, style, glossary.as_deref());
        let opts = GenOptions { max_tokens: prompt::cleanup_max_tokens(&base), temperature: 0.0 };
        match llm.generate(&prompt, &opts) {
            Ok(out) => match guard::check(&base, &out, &settings.cleanup.guard) {
                Ok(accepted) if snippet_cues_survive(settings, &base, &accepted) => {
                    text = dict.apply(&accepted);
                    used_llm = true;
                }
                Ok(_) => rejected = Some(Rejection::TooDifferent { edit_ratio: 1.0 }),
                Err(r) => rejected = Some(r),
            },
            Err(e) => log::warn!("LLM cleanup failed, using rules output: {e:#}"),
        }
    }
    let text = settings.snippets.expand(&style.tone.apply(&text));
    Cleaned { text, used_llm, rejected, press_enter }
}

fn snippet_cues_survive(settings: &Settings, before: &str, after: &str) -> bool {
    settings.snippets.cues_in(before).len() == settings.snippets.cues_in(after).len()
}

#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    NoSpeech,
    Inserted(String),
    /// Command Mode replaced the selection with this text.
    Replaced(String),
    /// Command Mode had nowhere to write, so the answer went to a notification
    /// (and the clipboard).
    Notified(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    pub outcome: Outcome,
    pub app: AppInfo,
    pub raw: String,
    pub used_llm: bool,
    pub rejected: Option<Rejection>,
    pub stt: Duration,
    pub cleanup: Duration,
}

pub struct Controller<D: Desktop> {
    pub settings: Settings,
    pub engines: Engines,
    pub desktop: D,
    pub history: Option<History>,
    pub paste_timing: PasteTiming,
}

impl<D: Desktop> Controller<D> {
    pub fn new(settings: Settings, engines: Engines, desktop: D, history: Option<History>) -> Self {
        let paste_timing = PasteTiming {
            restore_after: Duration::from_millis(settings.insertion.restore_delay_ms),
            ..PasteTiming::default()
        };
        Self { settings, engines, desktop, history, paste_timing }
    }

    fn transcribe(&mut self, samples: &[f32]) -> anyhow::Result<Option<String>> {
        let trimmed;
        let samples = match (&mut self.engines.vad, self.settings.stt.vad) {
            (Some(vad), true) => {
                let spans = vad.speech_spans(samples)?;
                let (Some(first), Some(last)) = (spans.first(), spans.last()) else { return Ok(None) };
                trimmed = samples[first.start..last.end].to_vec();
                &trimmed[..]
            }
            _ => samples,
        };
        let hints = SttHints {
            language: self.settings.stt.language.clone(),
            hotwords: self.settings.dictionary.hotwords(),
        };
        let use_multilingual = hints.language.as_deref().is_some_and(|l| !PARAKEET_LANGUAGES.contains(&l));
        let engine = match (&mut self.engines.multilingual, use_multilingual) {
            (Some(m), true) => m,
            _ => &mut self.engines.stt,
        };
        let text = engine.transcribe(samples, &hints)?;
        Ok(Some(text).filter(|t| !t.trim().is_empty()))
    }

    /// Processes one finished recording (16 kHz mono).
    pub fn process(&mut self, mode: Mode, samples: &[f32]) -> anyhow::Result<Report> {
        let app = self.desktop.frontmost_app();
        let started = Instant::now();
        let raw = self.transcribe(samples)?;
        let stt = started.elapsed();
        let Some(raw) = raw else {
            return Ok(Report {
                outcome: Outcome::NoSpeech,
                app,
                raw: String::new(),
                used_llm: false,
                rejected: None,
                stt,
                cleanup: Duration::ZERO,
            });
        };
        let cleanup_started = Instant::now();
        let (outcome, used_llm, rejected) = match mode {
            Mode::Dictation => self.dictate(&raw, &app)?,
            Mode::Command => (self.command(&raw)?, true, None),
        };
        let report = Report { outcome, app, raw, used_llm, rejected, stt, cleanup: cleanup_started.elapsed() };
        self.record(mode, &report);
        Ok(report)
    }

    fn dictate(&mut self, raw: &str, app: &AppInfo) -> anyhow::Result<(Outcome, bool, Option<Rejection>)> {
        let style = self.settings.styles.resolve(app);
        let llm = self.engines.llm.as_deref_mut().map(|l| l as &mut dyn TextGenerator);
        let cleaned = clean(raw, &self.settings, &style, llm);
        if cleaned.text.is_empty() && !cleaned.press_enter {
            return Ok((Outcome::NoSpeech, cleaned.used_llm, cleaned.rejected));
        }
        if !cleaned.text.is_empty() {
            self.insert(&cleaned.text)?;
        }
        if cleaned.press_enter {
            self.desktop.send(Keystroke::Enter)?;
        }
        Ok((Outcome::Inserted(cleaned.text), cleaned.used_llm, cleaned.rejected))
    }

    fn insert(&self, text: &str) -> anyhow::Result<()> {
        if self.settings.insertion.restore_clipboard {
            desktop::paste_text(&self.desktop, text, self.paste_timing)
        } else {
            self.desktop.set_clipboard_text(text)?;
            self.desktop.send(Keystroke::Paste)
        }
    }

    fn command(&mut self, instruction: &str) -> anyhow::Result<Outcome> {
        let Some(llm) = self.engines.llm.as_mut() else {
            anyhow::bail!("Command Mode needs a local LLM; download one in Settings > Models");
        };
        let selection = desktop::read_selection(&self.desktop, self.paste_timing);
        let field = self.desktop.focused_field();
        let context = field.as_ref().map(|f| tail(&f.value, 2000));
        let target = match (&selection, &context) {
            (Some(sel), _) => CommandTarget::Selection(sel),
            (None, Some(ctx)) => CommandTarget::Context(ctx),
            (None, None) => CommandTarget::None,
        };
        let base_len = match target {
            CommandTarget::Selection(s) => s.split_whitespace().count(),
            _ => 0,
        };
        let prompt = prompt::command_prompt(instruction, target);
        let opts = GenOptions { max_tokens: (base_len * 3).max(256), temperature: 0.0 };
        let out = llm.generate(&prompt, &opts)?;
        let out = strip_wrappers(&out);
        if selection.is_some() {
            self.insert(&out)?;
            Ok(Outcome::Replaced(out))
        } else if field.is_some() {
            self.insert(&out)?;
            Ok(Outcome::Inserted(out))
        } else {
            self.desktop.set_clipboard_text(&out)?;
            self.desktop.notify("OpenFlow", &out);
            Ok(Outcome::Notified(out))
        }
    }

    fn record(&self, mode: Mode, report: &Report) {
        let (Some(history), true) = (&self.history, self.settings.history.enabled) else { return };
        let text = match &report.outcome {
            Outcome::NoSpeech => return,
            Outcome::Inserted(t) | Outcome::Replaced(t) | Outcome::Notified(t) => t.clone(),
        };
        let entry = HistoryEntry {
            at_ms: SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0),
            mode,
            app: report.app.clone(),
            raw: report.raw.clone(),
            text,
            used_llm: report.used_llm,
            rejected: report.rejected.as_ref().map(|r| format!("{r:?}")),
            stt_ms: report.stt.as_millis() as u64,
            cleanup_ms: report.cleanup.as_millis() as u64,
        };
        if let Err(e) = history.append(&entry) {
            log::warn!("could not write history: {e:#}");
        }
    }
}

fn tail(text: &str, max_chars: usize) -> &str {
    let count = text.chars().count();
    if count <= max_chars {
        return text;
    }
    let skip = text.char_indices().nth(count - max_chars).map(|(i, _)| i).unwrap_or(0);
    &text[skip..]
}

fn strip_wrappers(out: &str) -> String {
    let t = out.trim();
    let t = t.strip_prefix("<text>").and_then(|r| r.strip_suffix("</text>")).unwrap_or(t);
    t.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::desktop::FieldSnapshot;
    use crate::mock::{FnGenerator, MockDesktop, ScriptedStt};
    use crate::snippets::Snippet;
    use crate::style::Tone;

    fn controller(stt: &[&str], llm: Option<Box<dyn TextGenerator>>) -> (Controller<MockDesktop>, MockDesktop) {
        let desktop = MockDesktop::default();
        let engines = Engines {
            stt: Box::new(ScriptedStt::new(stt.iter().copied())),
            multilingual: None,
            vad: None,
            llm,
        };
        let mut c = Controller::new(Settings::default(), engines, desktop.clone(), None);
        c.paste_timing = PasteTiming { settle: Duration::ZERO, restore_after: Duration::ZERO };
        (c, desktop)
    }

    fn fixed_llm(reply: &'static str) -> Option<Box<dyn TextGenerator>> {
        Some(Box::new(FnGenerator::new(move |_| reply.to_string())))
    }

    #[test]
    fn short_dictation_uses_rules_only_and_restores_clipboard() {
        let (mut c, d) = controller(&["Um, sounds good."], fixed_llm("SHOULD NOT BE USED"));
        d.set_clipboard_text("keep").unwrap();
        let r = c.process(Mode::Dictation, &[0.0; 16000]).unwrap();
        assert_eq!(r.outcome, Outcome::Inserted("Sounds good".into()));
        assert!(!r.used_llm);
        assert_eq!(d.pasted(), vec!["Sounds good".to_string()]);
        assert_eq!(d.clipboard_text().as_deref(), Some("keep"));
    }

    #[test]
    fn long_dictation_goes_through_llm_and_guard() {
        let raw = "um so I think we should uh ship the new build on friday morning";
        let (mut c, d) = controller(&[raw], fixed_llm("So I think we should ship the new build on Friday morning."));
        let r = c.process(Mode::Dictation, &[0.0; 16000]).unwrap();
        assert!(r.used_llm);
        assert_eq!(d.pasted(), vec!["So I think we should ship the new build on Friday morning".to_string()]);
    }

    #[test]
    fn llm_that_answers_is_rejected_for_rules_output() {
        let raw = "what is the best way to cook rice on a stove";
        let (mut c, d) = controller(
            &[raw],
            fixed_llm("The best way is to rinse the rice, add two cups of water per cup, simmer for eighteen minutes covered, then rest it off the heat for ten minutes before fluffing."),
        );
        let r = c.process(Mode::Dictation, &[0.0; 16000]).unwrap();
        assert!(!r.used_llm);
        assert!(matches!(r.rejected, Some(Rejection::TooLong { .. })));
        assert_eq!(d.pasted(), vec!["What is the best way to cook rice on a stove".to_string()]);
    }

    #[test]
    fn style_follows_frontmost_app() {
        let (mut c, d) = controller(&["Thanks for the update."], None);
        d.state().app = AppInfo { id: Some("com.apple.mail".into()), name: None, url: None };
        c.process(Mode::Dictation, &[0.0; 16000]).unwrap();
        d.state().app = AppInfo { id: Some("com.tinyspeck.slackmacgap".into()), name: None, url: None };
        c.engines.stt = Box::new(ScriptedStt::new(["Thanks for the update."]));
        c.process(Mode::Dictation, &[0.0; 16000]).unwrap();
        assert_eq!(d.pasted(), vec!["Thanks for the update.".to_string(), "Thanks for the update".to_string()]);
    }

    #[test]
    fn whole_snippet_cue_expands_without_llm() {
        let (mut c, d) = controller(&["Insert my calendar link."], fixed_llm("nope"));
        c.settings.snippets.items.push(Snippet { cue: "my calendar link".into(), text: "https://cal.com/vraj".into() });
        c.process(Mode::Dictation, &[0.0; 16000]).unwrap();
        assert_eq!(d.pasted(), vec!["https://cal.com/vraj".to_string()]);
    }

    #[test]
    fn dictionary_hotwords_reach_stt_and_terms_are_fixed() {
        let stt = ScriptedStt::new(["I deployed it to cooper netties."]);
        let seen = stt.seen_hints.clone();
        let (mut c, d) = controller(&[], None);
        c.engines.stt = Box::new(stt);
        c.settings.dictionary.upsert(crate::dictionary::DictEntry {
            term: "Kubernetes".into(),
            sounds_like: vec!["cooper netties".into()],
            source: Default::default(),
        });
        c.process(Mode::Dictation, &[0.0; 16000]).unwrap();
        assert_eq!(seen.lock().unwrap()[0].hotwords, vec!["Kubernetes".to_string()]);
        assert_eq!(d.pasted(), vec!["I deployed it to Kubernetes".to_string()]);
    }

    #[test]
    fn press_enter_sends_enter_after_paste() {
        let (mut c, d) = controller(&["Ship it, press enter."], None);
        c.process(Mode::Dictation, &[0.0; 16000]).unwrap();
        assert_eq!(d.pasted(), vec!["Ship it".to_string()]);
        assert_eq!(d.state().keys.last(), Some(&Keystroke::Enter));
    }

    #[test]
    fn empty_transcript_inserts_nothing() {
        let (mut c, d) = controller(&["  "], None);
        let r = c.process(Mode::Dictation, &[0.0; 16000]).unwrap();
        assert_eq!(r.outcome, Outcome::NoSpeech);
        assert!(d.pasted().is_empty());
    }

    #[test]
    fn non_parakeet_language_routes_to_multilingual_engine() {
        let (mut c, d) = controller(&["wrong engine"], None);
        c.engines.multilingual = Some(Box::new(ScriptedStt::new(["Namaste duniya."])));
        c.settings.stt.language = Some("hi".into());
        c.process(Mode::Dictation, &[0.0; 16000]).unwrap();
        assert_eq!(d.pasted(), vec!["Namaste duniya".to_string()]);
    }

    #[test]
    fn command_mode_replaces_selection() {
        let generator = FnGenerator::new(|p| {
            assert!(p.user.contains("<text>\nthis is a very long winded sentence\n</text>"));
            "A concise sentence.".to_string()
        });
        let (mut c, d) = controller(&["make this concise"], Some(Box::new(generator)));
        d.state().ax_selection = Some("this is a very long winded sentence".into());
        let r = c.process(Mode::Command, &[0.0; 16000]).unwrap();
        assert_eq!(r.outcome, Outcome::Replaced("A concise sentence.".into()));
        assert_eq!(d.pasted(), vec!["A concise sentence.".to_string()]);
    }

    #[test]
    fn command_mode_without_field_notifies_and_copies() {
        let (mut c, d) = controller(&["what is twelve times twelve"], fixed_llm("144"));
        let r = c.process(Mode::Command, &[0.0; 16000]).unwrap();
        assert_eq!(r.outcome, Outcome::Notified("144".into()));
        assert!(d.pasted().is_empty());
        assert_eq!(d.state().notifications, vec![("OpenFlow".to_string(), "144".to_string())]);
        assert_eq!(d.clipboard_text().as_deref(), Some("144"));
    }

    #[test]
    fn command_mode_with_field_but_no_selection_inserts_with_context() {
        let generator = FnGenerator::new(|p| {
            assert!(p.user.contains("<context>\nHi, can we move lunch?\n</context>"));
            "Sure, how about 1 pm?".to_string()
        });
        let (mut c, d) = controller(&["draft a reply"], Some(Box::new(generator)));
        d.state().field = Some(FieldSnapshot { value: "Hi, can we move lunch?".into(), selection: None });
        let r = c.process(Mode::Command, &[0.0; 16000]).unwrap();
        assert_eq!(r.outcome, Outcome::Inserted("Sure, how about 1 pm?".into()));
    }

    #[test]
    fn command_mode_without_llm_is_an_error() {
        let (mut c, _) = controller(&["make it shorter"], None);
        assert!(c.process(Mode::Command, &[0.0; 16000]).is_err());
    }

    #[test]
    fn history_records_inserted_text() {
        let dir = std::env::temp_dir().join(format!("openflow-ctl-history-{}", std::process::id()));
        let (mut c, _) = controller(&["Hello there."], None);
        c.history = Some(History::new(dir.join("h.jsonl"), 10));
        c.process(Mode::Dictation, &[0.0; 16000]).unwrap();
        let entries = c.history.as_ref().unwrap().load().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].raw, "Hello there.");
        assert_eq!(entries[0].text, "Hello there");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn split_press_enter_variants() {
        assert_eq!(split_press_enter("Ship it, press enter."), ("Ship it".into(), true));
        assert_eq!(split_press_enter("Press enter"), ("".into(), true));
        assert_eq!(split_press_enter("Don't press enter yet."), ("Don't press enter yet.".into(), false));
    }

    #[test]
    fn tone_is_used_for_casual_default() {
        assert_eq!(Settings::default().styles.default_tone, Tone::Casual);
    }
}

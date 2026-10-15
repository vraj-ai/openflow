//! User settings, persisted as one JSON file. Every field has a default, so a
//! settings file from an older version loads with new fields filled in.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::dictionary::Dictionary;
use crate::guard::GuardConfig;
use crate::hotkey::HotkeyConfig;
use crate::snippets::Snippets;
use crate::style::StyleSettings;
use crate::text::RuleSet;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Settings {
    pub hotkeys: HotkeyConfig,
    pub stt: SttSettings,
    pub cleanup: CleanupSettings,
    pub insertion: InsertionSettings,
    pub dictionary: Dictionary,
    pub snippets: Snippets,
    pub styles: StyleSettings,
    pub history: HistorySettings,
    pub learning: LearningSettings,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SttSettings {
    /// Catalog id of the primary speech model.
    pub model: String,
    /// Catalog id of the multilingual fallback used for languages the primary
    /// model does not cover.
    pub multilingual_model: String,
    /// Spoken language ("en", "hi"), or `None` to let the primary model decide.
    pub language: Option<String>,
    pub vad: bool,
}

impl Default for SttSettings {
    fn default() -> Self {
        Self {
            model: "parakeet-tdt-0.6b-v3".into(),
            multilingual_model: "whisper-large-v3-turbo".into(),
            language: None,
            vad: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CleanupSettings {
    pub rules: RuleSet,
    pub llm_enabled: bool,
    /// Catalog id of the cleanup LLM.
    pub llm_model: String,
    /// Utterances with fewer words skip the LLM and use rules only.
    pub min_words_for_llm: usize,
    pub guard: GuardConfig,
}

impl Default for CleanupSettings {
    fn default() -> Self {
        Self {
            rules: RuleSet::default(),
            llm_enabled: true,
            llm_model: "qwen3-4b-instruct-2507".into(),
            min_words_for_llm: 6,
            guard: GuardConfig::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct InsertionSettings {
    pub restore_clipboard: bool,
    pub restore_delay_ms: u64,
    /// Ending an utterance with "press enter" sends Enter after pasting.
    pub press_enter_command: bool,
}

impl Default for InsertionSettings {
    fn default() -> Self {
        Self { restore_clipboard: true, restore_delay_ms: 250, press_enter_command: true }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct HistorySettings {
    pub enabled: bool,
    pub max_entries: usize,
}

impl Default for HistorySettings {
    fn default() -> Self {
        Self { enabled: true, max_entries: 500 }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LearningSettings {
    /// Watch the text field after pasting and learn dictionary words from edits.
    pub auto_learn: bool,
    pub watch_ms: u64,
}

impl Default for LearningSettings {
    fn default() -> Self {
        Self { auto_learn: true, watch_ms: 10_000 }
    }
}

impl Settings {
    /// Missing file means defaults; a malformed file is an error so the user's
    /// settings are never silently replaced.
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(text) => Ok(serde_json::from_str(&text)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e.into()),
        }
    }

    /// Writes atomically through a temp file in the same directory.
    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(self)?)?;
        std::fs::rename(tmp, path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_file_fills_defaults() {
        let s: Settings = serde_json::from_str(r#"{"cleanup":{"llm_enabled":false}}"#).unwrap();
        assert!(!s.cleanup.llm_enabled);
        assert_eq!(s.cleanup.min_words_for_llm, 6);
        assert_eq!(s.stt.model, "parakeet-tdt-0.6b-v3");
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = std::env::temp_dir().join(format!("openflow-settings-{}", std::process::id()));
        let path = dir.join("settings.json");
        let mut s = Settings::default();
        s.cleanup.min_words_for_llm = 9;
        s.save(&path).unwrap();
        assert_eq!(Settings::load(&path).unwrap(), s);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn missing_file_is_default() {
        let s = Settings::load(Path::new("/nonexistent/openflow/settings.json")).unwrap();
        assert_eq!(s, Settings::default());
    }
}

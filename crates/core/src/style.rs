//! Per-app writing styles chosen from the frontmost app.

use serde::{Deserialize, Serialize};

use crate::desktop::AppInfo;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tone {
    Formal,
    #[default]
    Casual,
    VeryCasual,
    Excited,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum AppMatcher {
    /// Exact app id, or a prefix when it ends in `*` ("com.microsoft.*").
    AppId(String),
    /// Case-insensitive match on the display name.
    AppName(String),
    /// Host suffix of the browser URL ("mail.google.com", "slack.com").
    UrlHost(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StyleRule {
    pub matcher: AppMatcher,
    pub tone: Tone,
    #[serde(default)]
    pub instructions: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Style {
    pub tone: Tone,
    pub instructions: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct StyleSettings {
    pub default_tone: Tone,
    pub rules: Vec<StyleRule>,
}

impl Default for StyleSettings {
    fn default() -> Self {
        todo!()
    }
}

impl StyleSettings {
    pub fn resolve(&self, app: &AppInfo) -> Style {
        todo!("{app:?}")
    }
}

impl Tone {
    pub fn instruction(self) -> &'static str {
        todo!()
    }
    pub fn apply(self, text: &str) -> String {
        todo!("{text}")
    }
}

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
        let rule = |matcher: AppMatcher, tone: Tone| StyleRule { matcher, tone, instructions: None };
        use AppMatcher::{AppId, UrlHost};
        Self {
            default_tone: Tone::Casual,
            rules: vec![
                rule(AppId("com.apple.mail".into()), Tone::Formal),
                rule(AppId("com.microsoft.Outlook".into()), Tone::Formal),
                rule(AppId("com.microsoft.Word".into()), Tone::Formal),
                rule(AppId("notion.id".into()), Tone::Formal),
                rule(UrlHost("mail.google.com".into()), Tone::Formal),
                rule(UrlHost("docs.google.com".into()), Tone::Formal),
                rule(UrlHost("outlook.office.com".into()), Tone::Formal),
                rule(UrlHost("notion.so".into()), Tone::Formal),
                rule(AppId("com.tinyspeck.slackmacgap".into()), Tone::Casual),
                rule(AppId("slack.exe".into()), Tone::Casual),
                rule(AppId("slack".into()), Tone::Casual),
                rule(AppId("com.hnc.Discord".into()), Tone::Casual),
                rule(AppId("discord.exe".into()), Tone::Casual),
                rule(AppId("discord".into()), Tone::Casual),
                rule(UrlHost("app.slack.com".into()), Tone::Casual),
                rule(UrlHost("discord.com".into()), Tone::Casual),
                rule(AppId("com.apple.MobileSMS".into()), Tone::VeryCasual),
                rule(AppId("net.whatsapp.WhatsApp".into()), Tone::VeryCasual),
                rule(AppId("whatsapp.exe".into()), Tone::VeryCasual),
                rule(UrlHost("web.whatsapp.com".into()), Tone::VeryCasual),
            ],
        }
    }
}

impl StyleSettings {
    pub fn resolve(&self, app: &AppInfo) -> Style {
        for rule in &self.rules {
            if matches(&rule.matcher, app) {
                return Style { tone: rule.tone, instructions: rule.instructions.clone() };
            }
        }
        Style { tone: self.default_tone, instructions: None }
    }
}

impl Tone {
    pub fn instruction(self) -> &'static str {
        match self {
            Tone::Formal => "Use a professional tone with full sentences, correct capitalization and punctuation.",
            Tone::Casual => "Use a relaxed, conversational tone; keep the speaker's wording.",
            Tone::VeryCasual => {
                "Write like a quick text message: all lowercase, minimal punctuation, no trailing period."
            }
            Tone::Excited => "Use an upbeat, enthusiastic tone; exclamation marks are welcome where natural.",
        }
    }

    pub fn apply(self, text: &str) -> String {
        match self {
            Tone::Formal => formal(text),
            Tone::Casual => casual(text),
            Tone::VeryCasual => very_casual(text),
            Tone::Excited => excited(text),
        }
    }
}

fn same(a: &str, b: &str) -> bool {
    a.to_lowercase() == b.to_lowercase()
}

fn matches(matcher: &AppMatcher, app: &AppInfo) -> bool {
    match matcher {
        AppMatcher::AppId(value) => {
            let Some(id) = app.id.as_deref() else { return false };
            match value.strip_suffix('*') {
                Some(prefix) => id.to_lowercase().starts_with(&prefix.to_lowercase()),
                None => same(id, value),
            }
        }
        AppMatcher::AppName(value) => app.name.as_deref().is_some_and(|name| same(name, value)),
        AppMatcher::UrlHost(value) => app.url.as_deref().and_then(host_of).is_some_and(|host| {
            same(&host, value) || host.to_lowercase().ends_with(&format!(".{}", value.to_lowercase()))
        }),
    }
}

/// Host from a URL: scheme, userinfo, port, path and a leading `www.` removed.
fn host_of(url: &str) -> Option<String> {
    let trimmed = url.trim();
    let after_scheme = trimmed.split_once("://").map(|(_, rest)| rest).unwrap_or(trimmed);
    let authority = after_scheme.split(['/', '?', '#']).next().unwrap_or("");
    let hostport = authority.rsplit('@').next().unwrap_or(authority);
    let host = hostport.split(':').next().unwrap_or("").trim_start_matches("www.");
    (!host.is_empty()).then(|| host.to_string())
}

fn formal(text: &str) -> String {
    let t = text.trim_end();
    if t.is_empty() {
        return t.to_string();
    }
    if t.ends_with(['.', '!', '?', ':']) {
        return t.to_string();
    }
    let last_line = t.rsplit('\n').next().unwrap_or(t).trim_start();
    let digits = last_line.chars().take_while(|c| c.is_ascii_digit()).count();
    let is_list_item = digits > 0 && last_line[digits..].starts_with('.');
    if is_list_item {
        return t.to_string();
    }
    format!("{t}.")
}

fn single_sentence(t: &str) -> bool {
    !t.contains('\n') && !t[..t.len().saturating_sub(1)].contains(['.', '!', '?'])
}

fn casual(text: &str) -> String {
    let t = text.trim_end();
    if t.len() > 1 && t.ends_with('.') && single_sentence(t) {
        return t[..t.len() - 1].to_string();
    }
    t.to_string()
}

fn very_casual(text: &str) -> String {
    let t = text.trim_end();
    let t = t.strip_suffix('.').unwrap_or(t);
    let mut out = String::new();
    let mut chars = t.chars().peekable();
    let mut sentence_start = true;
    while let Some(ch) = chars.next() {
        if ch.is_alphanumeric() || ch == '\'' {
            let mut word = String::from(ch);
            while let Some(&next) = chars.peek() {
                if next.is_alphanumeric() || next == '\'' {
                    word.push(next);
                    chars.next();
                } else {
                    break;
                }
            }
            if sentence_start {
                out.push_str(&lower_sentence_initial(&word));
            } else {
                out.push_str(&word);
            }
            sentence_start = false;
        } else {
            if matches!(ch, '.' | '!' | '?' | '\n') {
                sentence_start = true;
            }
            out.push(ch);
        }
    }
    out
}

/// Lowercases only the first letter of a sentence-initial word, keeping "I",
/// acronyms ("NASA") and mid-sentence proper nouns intact.
fn lower_sentence_initial(word: &str) -> String {
    if word == "I" {
        return word.to_string();
    }
    let letters: Vec<char> = word.chars().filter(|c| c.is_alphabetic()).collect();
    if letters.len() >= 2 && letters.iter().all(|c| c.is_uppercase()) {
        return word.to_string();
    }
    let mut chars = word.chars();
    match chars.next() {
        Some(first) => first.to_lowercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

fn excited(text: &str) -> String {
    let t = text.trim_end();
    if t.len() > 1 && t.ends_with('.') && single_sentence(t) {
        return format!("{}!", &t[..t.len() - 1]);
    }
    t.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(id: Option<&str>, name: Option<&str>, url: Option<&str>) -> AppInfo {
        AppInfo {
            id: id.map(str::to_string),
            name: name.map(str::to_string),
            url: url.map(str::to_string),
        }
    }

    #[test]
    fn resolve_matches_bundle_id() {
        let style = StyleSettings::default().resolve(&app(Some("com.apple.mail"), None, None));
        assert_eq!(style, Style { tone: Tone::Formal, instructions: None });
    }

    #[test]
    fn resolve_matches_url_host_with_www_and_path() {
        let style =
            StyleSettings::default().resolve(&app(None, None, Some("https://www.docs.google.com/document/d/abc")));
        assert_eq!(style.tone, Tone::Formal);
    }

    #[test]
    fn resolve_matches_url_host_suffix() {
        let style = StyleSettings::default().resolve(&app(Some("com.google.Chrome"), None, Some("https://app.slack.com/client/T1")));
        assert_eq!(style.tone, Tone::Casual);
    }

    #[test]
    fn resolve_falls_back_to_default_for_unlisted_apps() {
        let style = StyleSettings::default().resolve(&app(Some("com.google.Chrome"), None, None));
        assert_eq!(style, Style { tone: Tone::Casual, instructions: None });
    }

    #[test]
    fn resolve_matches_id_prefix_and_app_name() {
        let settings = StyleSettings {
            default_tone: Tone::Casual,
            rules: vec![
                StyleRule {
                    matcher: AppMatcher::AppId("com.microsoft.*".into()),
                    tone: Tone::Formal,
                    instructions: None,
                },
                StyleRule {
                    matcher: AppMatcher::AppName("Notes".into()),
                    tone: Tone::Excited,
                    instructions: Some("be brief".into()),
                },
            ],
        };
        assert_eq!(settings.resolve(&app(Some("com.microsoft.word"), None, None)).tone, Tone::Formal);
        let noted = settings.resolve(&app(None, Some("notes"), None));
        assert_eq!(noted, Style { tone: Tone::Excited, instructions: Some("be brief".into()) });
    }

    #[test]
    fn formal_adds_a_period_but_not_after_a_list_item() {
        assert_eq!(Tone::Formal.apply("we should meet tuesday"), "we should meet tuesday.");
        assert_eq!(Tone::Formal.apply("all done!"), "all done!");
        assert_eq!(Tone::Formal.apply("1. buy milk\n2. call bob"), "1. buy milk\n2. call bob");
    }

    #[test]
    fn casual_drops_a_lone_trailing_period() {
        assert_eq!(Tone::Casual.apply("Sounds good."), "Sounds good");
        assert_eq!(Tone::Casual.apply("Sounds good. See you there."), "Sounds good. See you there.");
        assert_eq!(Tone::Casual.apply("Sounds good"), "Sounds good");
    }

    #[test]
    fn very_casual_lowercases_sentence_starts_and_drops_the_period() {
        assert_eq!(Tone::VeryCasual.apply("See you there."), "see you there");
        assert_eq!(Tone::VeryCasual.apply("Hi Bob. Meet at NASA tomorrow."), "hi Bob. meet at NASA tomorrow");
        assert_eq!(Tone::VeryCasual.apply("I will be there."), "I will be there");
    }

    #[test]
    fn excited_swaps_a_lone_final_period_for_a_bang() {
        assert_eq!(Tone::Excited.apply("That is great."), "That is great!");
        assert_eq!(Tone::Excited.apply("One. Two."), "One. Two.");
        assert_eq!(Tone::Excited.apply("Already excited!"), "Already excited!");
    }

    #[test]
    fn instruction_text_is_stable() {
        assert_eq!(
            Tone::Formal.instruction(),
            "Use a professional tone with full sentences, correct capitalization and punctuation."
        );
        assert_eq!(Tone::Casual.instruction(), "Use a relaxed, conversational tone; keep the speaker's wording.");
        assert_eq!(
            Tone::VeryCasual.instruction(),
            "Write like a quick text message: all lowercase, minimal punctuation, no trailing period."
        );
        assert_eq!(
            Tone::Excited.instruction(),
            "Use an upbeat, enthusiastic tone; exclamation marks are welcome where natural."
        );
    }
}

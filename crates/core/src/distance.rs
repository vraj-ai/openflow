//! Word-level metrics shared by the guardrail and the eval harness.

/// Lowercased words with surrounding punctuation stripped (inner apostrophes and
/// hyphens kept). "Hello, World!" -> ["hello", "world"].
pub fn normalize_words(text: &str) -> Vec<String> {
    text.split_whitespace()
        .map(|w| {
            w.trim_matches(|c: char| !c.is_alphanumeric() && c != '\'' && c != '-').to_lowercase()
        })
        .filter(|w| !w.is_empty())
        .collect()
}

/// Levenshtein distance over word tokens.
pub fn word_edit_distance<S: AsRef<str>>(a: &[S], b: &[S]) -> usize {
    let a: Vec<&str> = a.iter().map(|s| s.as_ref()).collect();
    let b: Vec<&str> = b.iter().map(|s| s.as_ref()).collect();
    levenshtein(&a, &b)
}

/// Word error rate of `hypothesis` against `reference` after `normalize_words`.
/// 0.0 when both are empty; 1.0 when only the reference is empty.
pub fn wer(reference: &str, hypothesis: &str) -> f64 {
    let r = normalize_words(reference);
    let h = normalize_words(hypothesis);
    if r.is_empty() {
        return if h.is_empty() { 0.0 } else { 1.0 };
    }
    word_edit_distance(&r, &h) as f64 / r.len() as f64
}

/// Character error rate on lowercased text with whitespace collapsed.
pub fn cer(reference: &str, hypothesis: &str) -> f64 {
    let r: Vec<char> = collapse(reference).chars().collect();
    let h: Vec<char> = collapse(hypothesis).chars().collect();
    if r.is_empty() {
        return if h.is_empty() { 0.0 } else { 1.0 };
    }
    levenshtein(&r, &h) as f64 / r.len() as f64
}

fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// Levenshtein with two rolling rows, O(n*m) time and O(m) space.
fn levenshtein<T: PartialEq>(a: &[T], b: &[T]) -> usize {
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0usize; b.len() + 1];
    for (i, x) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, y) in b.iter().enumerate() {
            let cost = usize::from(x != y);
            cur[j + 1] = (prev[j] + cost).min(prev[j + 1] + 1).min(cur[j] + 1);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_case_and_surrounding_punctuation() {
        assert_eq!(normalize_words("Hello, World!"), vec!["hello", "world"]);
        assert_eq!(normalize_words("\"Kubernetes\", — yes"), vec!["kubernetes", "yes"]);
        assert_eq!(normalize_words("don't stop"), vec!["don't", "stop"]);
    }

    #[test]
    fn word_edit_distance_counts_substitutions() {
        assert_eq!(word_edit_distance(&["a", "b"], &["a", "b"]), 0);
        assert_eq!(word_edit_distance(&["a", "b"], &["a"]), 1);
        assert_eq!(word_edit_distance(&["a"], &["a", "b", "c"]), 2);
    }

    #[test]
    fn wer_identical_is_zero() {
        assert_eq!(wer("the cat sat", "the cat sat"), 0.0);
    }

    #[test]
    fn wer_ignores_case_and_punctuation() {
        assert_eq!(wer("Hello, World!", "hello world"), 0.0);
    }

    #[test]
    fn wer_one_word_wrong_is_one_third() {
        let w = wer("the cat sat", "the bat sat");
        assert!((w - 1.0 / 3.0).abs() < 1e-9, "{w}");
    }

    #[test]
    fn wer_empty_reference_rules() {
        assert_eq!(wer("", ""), 0.0);
        assert_eq!(wer("", "hello"), 1.0);
    }

    #[test]
    fn cer_is_character_level() {
        assert_eq!(cer("Hello", "hello"), 0.0);
        assert_eq!(cer("a  b", "a b"), 0.0);
        let c = cer("abc", "abd");
        assert!((c - 1.0 / 3.0).abs() < 1e-9, "{c}");
    }
}

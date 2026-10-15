//! Final normalisation pass: spacing, punctuation spacing, capitalisation.

/// Collapses space runs, trims, fixes punctuation spacing, capitalises sentence
/// starts and turns a bare "i" into "I". Never adds a final period. Idempotent.
pub fn tidy(text: &str) -> String {
    let s = collapse_spaces(text);
    let s = drop_redundant_commas(&s);
    let s = space_before_punct(&s);
    let s = trim_lines(&s);
    let s = uppercase_sentences(&s);
    lone_i(&s)
}

/// Space/tab runs become one space; newlines are left alone.
fn collapse_spaces(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut run = false;
    for c in s.chars() {
        if c == ' ' || c == '\t' {
            if !run {
                out.push(' ');
                run = true;
            }
        } else {
            run = false;
            out.push(c);
        }
    }
    out
}

/// A comma directly followed by another mark is the weaker one: drop it.
fn drop_redundant_commas(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == ',' {
            let mut j = i + 1;
            while j < chars.len() && (chars[j] == ' ' || chars[j] == '\t') {
                j += 1;
            }
            if j < chars.len() && matches!(chars[j], ',' | '.' | '!' | '?' | ';' | ':') {
                i += 1;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

fn space_before_punct(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(c, ',' | '.' | '!' | '?' | ';' | ':') {
            while out.ends_with([' ', '\t']) {
                out.pop();
            }
        }
        out.push(c);
    }
    out
}

fn trim_lines(s: &str) -> String {
    let joined: String = s.split('\n').map(|l| l.trim_end_matches([' ', '\t'])).collect::<Vec<_>>().join("\n");
    joined.trim().to_string()
}

/// Uppercases the first letter of the text and of every sentence after `. ! ?`
/// or a newline. Digits and closing brackets do not close a sentence start.
fn uppercase_sentences(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut cap = true;
    for (i, &c) in chars.iter().enumerate() {
        if c.is_alphabetic() {
            if cap {
                out.extend(c.to_uppercase());
                cap = false;
            } else {
                out.push(c);
            }
            continue;
        }
        if c == '\n' {
            cap = true;
        } else if matches!(c, '.' | '!' | '?') {
            // Only end a sentence when whitespace (or the end) follows, so
            // "3.5" and "e.g." do not start a new sentence.
            cap = chars.get(i + 1).is_none_or(|n| n.is_whitespace());
        }
        out.push(c);
    }
    out
}

fn lone_i(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    for (i, &c) in chars.iter().enumerate() {
        if c == 'i' {
            let prev_ok = i == 0 || !chars[i - 1].is_alphanumeric();
            let next_ok = chars.get(i + 1).is_none_or(|n| !n.is_alphanumeric() && *n != '\'');
            if prev_ok && next_ok {
                out.push('I');
                continue;
            }
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collapses_spaces_and_capitalizes() {
        assert_eq!(tidy("  hello   world  "), "Hello world");
    }

    #[test]
    fn removes_spaces_before_punctuation() {
        assert_eq!(tidy("hi , there ."), "Hi, there.");
    }

    #[test]
    fn capitalizes_every_sentence() {
        assert_eq!(tidy("hello. how are you? fine!"), "Hello. How are you? Fine!");
    }

    #[test]
    fn capitalizes_after_newline() {
        assert_eq!(tidy("first line  \nsecond line "), "First line\nSecond line");
    }

    #[test]
    fn lone_i_becomes_capital() {
        assert_eq!(tidy("i think i am"), "I think I am");
        assert_eq!(tidy("i'll stay"), "I'll stay");
    }

    #[test]
    fn redundant_commas_collapse_to_the_stronger_mark() {
        assert_eq!(tidy("hello , , world"), "Hello, world");
        assert_eq!(tidy("hello ,. world"), "Hello. World");
    }

    #[test]
    fn decimal_points_do_not_start_a_sentence() {
        assert_eq!(tidy("it is 3.5 kg"), "It is 3.5 kg");
    }

    #[test]
    fn does_not_add_a_final_period() {
        assert_eq!(tidy("no period here"), "No period here");
    }

    #[test]
    fn empty_and_unicode() {
        assert_eq!(tidy(""), "");
        assert_eq!(tidy("über alles"), "Über alles");
        assert_eq!(tidy("café  bar"), "Café bar");
    }

    #[test]
    fn pipeline_of_all_rules() {
        let rules = crate::text::RuleSet::default();
        assert_eq!(
            crate::text::apply_rules("Um, so, let's meet on Monday, no wait, Tuesday.", &rules),
            "So, let's meet on Tuesday."
        );
        assert_eq!(
            crate::text::apply_rules("Hi team, new paragraph, the launch is Friday.", &rules),
            "Hi team,\n\nThe launch is Friday."
        );
        assert_eq!(
            crate::text::apply_rules("My goals are first ship the app, second write the docs.", &rules),
            "My goals are:\n1. Ship the app\n2. Write the docs"
        );
    }

    #[test]
    fn idempotent_on_every_spec_example() {
        let examples = [
            "  hello   world  ",
            "hi , there .",
            "hello. how are you? fine!",
            "first line  \nsecond line ",
            "i think i am",
            "hello , , world",
            "hello ,. world",
            "it is 3.5 kg",
            "Um, so I think we should, uh, meet.",
            "Uh-huh, sounds good.",
            "Hmm.",
            "Let's meet on Monday, no wait, Tuesday.",
            "Let's meet at 3 pm no wait at 4 pm.",
            "Send it to John, sorry, Jane.",
            "The budget is 5 million, I mean 6 million.",
            "I'll bring the cake. Scratch that. I'll bring cookies.",
            "Buy milk, actually make that oat milk.",
            "I mean it when I say thanks.",
            "No wait is too long.",
            "Hi team, new paragraph, the launch is Friday.",
            "Hi, new line, how are you?",
            "My goals are first ship the app, second write the docs, and third rest.",
            "First of all, thanks.",
            "At first I hesitated.",
            "Number one, eggs. Number two, milk.",
            "My goals are first a, second b, third c. Thanks all.",
            "",
            "über alles",
        ];
        for e in examples {
            assert_eq!(tidy(&tidy(e)), tidy(e), "not idempotent: {e:?}");
        }
    }
}

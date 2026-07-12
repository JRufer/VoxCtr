use std::collections::HashMap;

use regex::Regex;

// ── Filler removal ────────────────────────────────────────────────────────────

static FILLER_PATTERN: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();

fn filler_re() -> &'static Regex {
    FILLER_PATTERN.get_or_init(|| {
        Regex::new(r"(?i)\b(uh+|um+|hmm+|er+|ah+|ugh+|mhm+)\b,?\s*").unwrap()
    })
}

pub fn remove_fillers(text: &str) -> String {
    filler_re().replace_all(text, "").trim().to_string()
}

// ── Spoken punctuation ────────────────────────────────────────────────────────

static PUNCT_WORDS: &[(&str, &str)] = &[
    ("period",           ". "),
    ("full stop",        ". "),
    ("comma",            ", "),
    ("question mark",    "? "),
    ("exclamation mark", "! "),
    ("exclamation point","! "),
    ("colon",            ": "),
    ("semicolon",        "; "),
    ("open bracket",     "("),
    ("close bracket",    ")"),
    ("open paren",       "("),
    ("close paren",      ")"),
    ("new line",         "\n"),
    ("new paragraph",    "\n\n"),
    ("tab",              "\t"),
    ("dash",             " — "),
    ("hyphen",           "-"),
    ("ellipsis",         "..."),
    ("slash",            "/"),
    ("backslash",        "\\"),
    ("at sign",          "@"),
    ("hash",             "#"),
    ("percent",          "%"),
    ("ampersand",        "&"),
    ("asterisk",         "*"),
    ("plus sign",        "+"),
    ("equals sign",      "="),
    ("less than",        "<"),
    ("greater than",     ">"),
];

// Compiled once at first call; reused for every subsequent transcription.
static PUNCT_REGEX_TABLE: std::sync::OnceLock<Vec<(Regex, &'static str)>> =
    std::sync::OnceLock::new();
static DOUBLE_SPACE_RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
static LIST_RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();

fn punct_regex_table() -> &'static [(Regex, &'static str)] {
    PUNCT_REGEX_TABLE.get_or_init(|| {
        PUNCT_WORDS
            .iter()
            .filter_map(|(word, repl)| {
                let pattern = format!(r"(?i)\b{}\b", regex::escape(word));
                Regex::new(&pattern).ok().map(|re| (re, *repl))
            })
            .collect()
    })
}

fn double_space_re() -> &'static Regex {
    DOUBLE_SPACE_RE.get_or_init(|| Regex::new(r" {2,}").unwrap())
}

fn list_re() -> &'static Regex {
    LIST_RE.get_or_init(|| {
        Regex::new(
            r"(?i)\b(first(?:ly)?|second(?:ly)?|third(?:ly)?|fourth(?:ly)?|fifth(?:ly)?|finally)\b,?\s*",
        )
        .unwrap()
    })
}

pub fn apply_spoken_punctuation(text: &str) -> String {
    let mut result = text.to_string();
    for (re, replacement) in punct_regex_table() {
        result = re.replace_all(&result, *replacement).to_string();
    }
    double_space_re().replace_all(&result, " ").trim().to_string()
}

// ── Auto list formatting ──────────────────────────────────────────────────────

pub fn auto_format_lists(text: &str) -> String {
    let re = list_re();
    if !re.is_match(text) {
        return text.to_string();
    }

    let mut counter = 1u32;
    re.replace_all(text, |_caps: &regex::Captures| {
            let prefix = format!("\n{}. ", counter);
            counter += 1;
            prefix
        })
        .trim()
        .to_string()
}

// ── Snippet expansion ─────────────────────────────────────────────────────────

// Shared with voxctrl-tts, which applies the same expansion to text before
// speaking it. See voxctrl-text for the implementation.
pub use voxctrl_text::expand_snippets;

// ── Code mode ─────────────────────────────────────────────────────────────────

pub fn apply_code_mode(text: &str) -> String {
    // Replace spaces between words with underscores (snake_case by default)
    // Words that look like operators are preserved
    let operator_re = Regex::new(r"\b(equals|plus|minus|times|divided by|modulo)\b").unwrap();
    let s = operator_re
        .replace_all(text, |caps: &regex::Captures| -> String {
            match caps[0].to_lowercase().as_str() {
                "equals" => "=".into(),
                "plus" => "+".into(),
                "minus" => "-".into(),
                "times" => "*".into(),
                "divided by" => "/".into(),
                "modulo" => "%".into(),
                _ => caps[0].to_string(),
            }
        })
        .to_string();

    // Convert "camel case" spoken as separate words: "my function" → "myFunction"
    // Heuristic: only when the phrase starts with a lowercase letter
    let words: Vec<&str> = s.split_whitespace().collect();
    if words.len() > 1 && words[0].chars().next().map(|c| c.is_lowercase()).unwrap_or(false) {
        let camel = words
            .iter()
            .enumerate()
            .map(|(i, w)| {
                if i == 0 {
                    w.to_string()
                } else {
                    let mut c = w.chars();
                    match c.next() {
                        None => String::new(),
                        Some(f) => f.to_uppercase().to_string() + c.as_str(),
                    }
                }
            })
            .collect::<Vec<_>>()
            .join("");
        return camel;
    }

    s
}

// Shared with voxctrl-tts, which applies the same fuzzy correction to text
// before speaking it. See voxctrl-text for the implementation.
pub use voxctrl_text::{correct_custom_vocabulary, levenshtein_distance};

// ── Full post-processing pipeline ─────────────────────────────────────────────

pub fn is_silence_hallucination(text: &str) -> bool {
    let cleaned = text
        .trim()
        .trim_end_matches(|c: char| c.is_ascii_punctuation())
        .trim()
        .to_lowercase();
    cleaned == "thank you" || cleaned == "thanks for watching" || cleaned == "thank you for watching"
}

#[derive(Debug, Clone)]
pub struct PostProcessConfig {
    pub remove_fillers: bool,
    pub spoken_punctuation: bool,
    pub auto_format_lists: bool,
    pub apply_snippets: bool,
    pub snippets: HashMap<String, String>,
    pub code_mode: bool,
    pub custom_vocabulary: Vec<String>,
}

pub fn run_pipeline(text: &str, cfg: &PostProcessConfig) -> String {
    let mut s = text.to_string();

    if cfg.remove_fillers {
        s = remove_fillers(&s);
    }
    if cfg.spoken_punctuation {
        s = apply_spoken_punctuation(&s);
    }
    if cfg.auto_format_lists {
        s = auto_format_lists(&s);
    }
    if cfg.apply_snippets {
        s = expand_snippets(&s, &cfg.snippets);
    }
    if !cfg.custom_vocabulary.is_empty() {
        s = correct_custom_vocabulary(&s, &cfg.custom_vocabulary);
    }
    if cfg.code_mode {
        s = apply_code_mode(&s);
    }

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spoken_punct_regex_table_cached() {
        // Calling twice must return the exact same slice address (OnceLock).
        let p1 = punct_regex_table().as_ptr();
        let p2 = punct_regex_table().as_ptr();
        assert_eq!(p1, p2, "punct_regex_table must return the same compiled Vec");
    }

    #[test]
    fn test_auto_format_lists_regex_cached() {
        let p1 = list_re() as *const _;
        let p2 = list_re() as *const _;
        assert_eq!(p1, p2, "list_re must return the same compiled Regex");
    }

    #[test]
    fn test_double_space_regex_cached() {
        let p1 = double_space_re() as *const _;
        let p2 = double_space_re() as *const _;
        assert_eq!(p1, p2, "double_space_re must return the same compiled Regex");
    }

    #[test]
    fn test_spoken_punct_correctness() {
        // The regex matches only the spoken word; the preceding space is preserved.
        assert_eq!(
            apply_spoken_punctuation("Hello period world"),
            "Hello . world"
        );
        assert_eq!(
            apply_spoken_punctuation("yes comma no"),
            "yes , no"
        );
        assert_eq!(
            apply_spoken_punctuation("what question mark"),
            "what ?"
        );
    }

    #[test]
    fn test_silence_hallucinations() {
        assert!(is_silence_hallucination("Thank you."));
        assert!(is_silence_hallucination("Thank you!"));
        assert!(is_silence_hallucination("thank you"));
        assert!(is_silence_hallucination("Thanks for watching"));
        assert!(is_silence_hallucination("Thank you for watching."));
        
        // These should NOT be matched as silence hallucinations
        assert!(!is_silence_hallucination("Thank you for your help"));
        assert!(!is_silence_hallucination("Thank you very much"));
        assert!(!is_silence_hallucination("Hello world"));
    }

    #[test]
    fn filler_removal() {
        assert_eq!(remove_fillers("Hello uh world"), "Hello world");
        assert_eq!(remove_fillers("um, this is a test"), "this is a test");
    }

    #[test]
    fn spoken_punct() {
        let result = apply_spoken_punctuation("Hello world period");
        assert!(result.contains(". ") || result.ends_with('.'));
    }

    // Full snippet-expansion / custom-vocabulary coverage now lives in
    // voxctrl-text (shared with voxctrl-tts). This is a thin smoke test
    // confirming the re-exports are wired up correctly from this module.
    #[test]
    fn snippet_and_vocab_reexports_work() {
        let mut snips = HashMap::new();
        snips.insert("addr".into(), "123 Main Street".into());
        assert_eq!(expand_snippets("Send to addr please", &snips), "Send to 123 Main Street please");

        let vocab = vec!["Rufer".to_string()];
        assert_eq!(correct_custom_vocabulary("RUFER is here", &vocab), "Rufer is here");
    }
}

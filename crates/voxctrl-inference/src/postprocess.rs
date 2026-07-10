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

pub fn expand_snippets(text: &str, snippets: &HashMap<String, String>) -> String {
    if snippets.is_empty() {
        return text.to_string();
    }
    let mut result = text.to_string();
    for (trigger, expansion) in snippets {
        let pattern = format!(r"(?i)\b{}\b", regex::escape(trigger));
        if let Ok(re) = Regex::new(&pattern) {
            result = re.replace_all(&result, expansion.as_str()).to_string();
        }
    }
    result
}

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

fn levenshtein_distance(s1: &str, s2: &str) -> usize {
    let s1_chars: Vec<char> = s1.chars().collect();
    let s2_chars: Vec<char> = s2.chars().collect();
    let len1 = s1_chars.len();
    let len2 = s2_chars.len();

    let mut dp = vec![vec![0; len2 + 1]; len1 + 1];

    for i in 0..=len1 {
        dp[i][0] = i;
    }
    for j in 0..=len2 {
        dp[0][j] = j;
    }

    for i in 1..=len1 {
        for j in 1..=len2 {
            if s1_chars[i - 1] == s2_chars[j - 1] {
                dp[i][j] = dp[i - 1][j - 1];
            } else {
                dp[i][j] = 1 + std::cmp::min(
                    dp[i - 1][j - 1], // substitution
                    std::cmp::min(
                        dp[i - 1][j], // deletion
                        dp[i][j - 1], // insertion
                    )
                );
            }
        }
    }

    dp[len1][len2]
}

pub fn correct_custom_vocabulary(text: &str, custom_vocab: &[String]) -> String {
    if custom_vocab.is_empty() {
        return text.to_string();
    }

    let mut multi_word: Vec<&String> = Vec::new();
    let mut single_word: Vec<&String> = Vec::new();
    for vocab_word in custom_vocab {
        if vocab_word.trim().is_empty() {
            continue;
        }
        if vocab_word.contains(' ') {
            multi_word.push(vocab_word);
        } else {
            single_word.push(vocab_word);
        }
    }

    let mut result = text.to_string();

    // 1. Process multi-word phrases first (longest first)
    multi_word.sort_by(|a, b| b.len().cmp(&a.len()));

    if !multi_word.is_empty() {
        let re_word = Regex::new(r"[a-zA-Z0-9'\-]+").unwrap();
        for phrase in multi_word {
            let phrase_lower = phrase.to_lowercase();
            let phrase_words: Vec<&str> = phrase_lower.split_whitespace().collect();
            let n = phrase_words.len();
            if n == 0 {
                continue;
            }

            let mut replaced_any = true;
            while replaced_any {
                replaced_any = false;
                let tokens: Vec<_> = re_word.find_iter(&result).collect();
                if tokens.len() < n {
                    break;
                }

                for i in 0..=(tokens.len() - n) {
                    let start_tok = &tokens[i];
                    let end_tok = &tokens[i + n - 1];
                    let start_byte = start_tok.start();
                    let end_byte = end_tok.end();

                    let candidate_str = &result[start_byte..end_byte];
                    let candidate_words: Vec<&str> = candidate_str
                        .split_whitespace()
                        .map(|w| w.trim_matches(|c: char| c.is_ascii_punctuation()))
                        .filter(|w| !w.is_empty())
                        .collect();

                    if candidate_words.len() != n {
                        continue;
                    }

                    let candidate_norm = candidate_words.join(" ").to_lowercase();
                    let dist = levenshtein_distance(&candidate_norm, &phrase_lower);

                    let phrase_len = phrase_lower.chars().count();
                    let max_allowed = if phrase_len <= 4 {
                        0
                    } else if phrase_len <= 8 {
                        1
                    } else {
                        2
                    };

                    if dist <= max_allowed {
                        result.replace_range(start_byte..end_byte, phrase);
                        replaced_any = true;
                        break;
                    }
                }
            }
        }
    }

    // 2. Process single-word corrections
    if !single_word.is_empty() {
        let re_word = match Regex::new(r"[a-zA-Z0-9'\-]+") {
            Ok(re) => re,
            Err(_) => return result,
        };

        result = re_word.replace_all(&result, |caps: &regex::Captures| {
            let matched = caps.get(0).unwrap().as_str();

            let mut best_match: Option<&str> = None;
            let mut best_dist = usize::MAX;

            let matched_lower = matched.to_lowercase();

            for vocab_word in &single_word {
                let vocab_lower = vocab_word.to_lowercase();
                let len = vocab_lower.chars().count();

                if matched_lower == vocab_lower {
                    best_match = Some(vocab_word.as_str());
                    break;
                }

                let dist = levenshtein_distance(&matched_lower, &vocab_lower);

                let max_allowed = if len <= 3 {
                    0
                } else if len == 4 {
                    1
                } else {
                    2
                };

                if dist <= max_allowed && dist < best_dist {
                    best_dist = dist;
                    best_match = Some(vocab_word.as_str());
                }
            }

            if let Some(replacement) = best_match {
                replacement.to_string()
            } else {
                matched.to_string()
            }
        }).to_string();
    }

    result
}

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

    #[test]
    fn snippet_expand() {
        let mut snips = HashMap::new();
        snips.insert("addr".into(), "123 Main Street".into());
        let result = expand_snippets("Send to addr please", &snips);
        assert_eq!(result, "Send to 123 Main Street please");
    }

    #[test]
    fn custom_vocab_correction() {
        let vocab = vec![
            "Waylin".to_string(),
            "Rufer".to_string(),
            "Enola".to_string(),
            "Kenz".to_string(),
            "Vox Ctrl".to_string(),
        ];
        
        // Exact case-insensitive matches should capitalize correctly
        assert_eq!(correct_custom_vocabulary("hello waylin", &vocab), "hello Waylin");
        assert_eq!(correct_custom_vocabulary("RUFER is here", &vocab), "Rufer is here");
        assert_eq!(correct_custom_vocabulary("kenz", &vocab), "Kenz");
        
        // Fuzzy matches (edit distance <= 2 for long words, <= 1 for mid-length)
        assert_eq!(correct_custom_vocabulary("Hello Waylan!", &vocab), "Hello Waylin!");
        assert_eq!(correct_custom_vocabulary("this is Enoll", &vocab), "this is Enola");
        assert_eq!(correct_custom_vocabulary("my friend kens", &vocab), "my friend Kenz");
        
        // Short words and distant words should not trigger false positives
        assert_eq!(correct_custom_vocabulary("in", &vocab), "in");
        assert_eq!(correct_custom_vocabulary("hello world", &vocab), "hello world");

        // Multi-word phrase corrections
        assert_eq!(correct_custom_vocabulary("hello vox ctrl", &vocab), "hello Vox Ctrl");
        assert_eq!(correct_custom_vocabulary("using vox control here", &vocab), "using Vox Ctrl here");
        assert_eq!(correct_custom_vocabulary("welcome to vox crtl!", &vocab), "welcome to Vox Ctrl!");
    }
}

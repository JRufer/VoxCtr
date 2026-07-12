//! Shared text-processing helpers used by both the speech-recognition
//! post-processing pipeline (`voxctrl-inference`) and the TTS engine
//! (`voxctrl-tts`). Both crates apply the same snippet expansion and
//! custom-vocabulary fuzzy correction to user text — this crate is the
//! single source of truth for that logic so a fix or tuning change only
//! needs to happen once.

use std::collections::HashMap;

use regex::Regex;

/// Classic Levenshtein (edit) distance between two strings, computed over
/// `char`s rather than bytes so it works correctly with non-ASCII input.
pub fn levenshtein_distance(s1: &str, s2: &str) -> usize {
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

/// Replaces short trigger words/phrases in `text` with their configured
/// expansions (case-insensitive, word-boundary matched). Used for both
/// dictation snippets (STT output) and spoken-text shortcuts (TTS input).
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

/// Fuzzy-corrects occurrences of `custom_vocab` entries in `text` using
/// Levenshtein distance, so phonetic mis-transcriptions/mis-readings of
/// proper nouns and domain-specific terms get normalized back to the
/// configured spelling.
///
/// Multi-word phrases are corrected first (longest phrase first, so a
/// longer match "wins" over a shorter one contained within it), followed
/// by single-word corrections. The allowed edit distance scales with word
/// length: exact match only for 1-3 chars, distance <= 1 for mid-length
/// words, distance <= 2 for longer words.
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
        let re_word = match Regex::new(r"[a-zA-Z0-9'\-]+") {
            Ok(re) => re,
            Err(_) => return result,
        };
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levenshtein_identical_strings_is_zero() {
        assert_eq!(levenshtein_distance("hello", "hello"), 0);
    }

    #[test]
    fn levenshtein_basic_cases() {
        assert_eq!(levenshtein_distance("kitten", "sitting"), 3);
        assert_eq!(levenshtein_distance("", "abc"), 3);
        assert_eq!(levenshtein_distance("abc", ""), 3);
    }

    #[test]
    fn snippet_expand() {
        let mut snips = HashMap::new();
        snips.insert("addr".into(), "123 Main Street".into());
        let result = expand_snippets("Send to addr please", &snips);
        assert_eq!(result, "Send to 123 Main Street please");
    }

    #[test]
    fn snippet_expand_empty_map_is_noop() {
        let snips = HashMap::new();
        assert_eq!(expand_snippets("unchanged text", &snips), "unchanged text");
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

    #[test]
    fn custom_vocab_correction_empty_vocab_is_noop() {
        assert_eq!(correct_custom_vocabulary("hello world", &[]), "hello world");
    }
}

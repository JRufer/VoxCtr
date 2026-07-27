//! eSpeak-NG phoneme frontend for Inflect-Micro-v2.
//!
//! Inflect keeps its English grapheme-to-phoneme step outside the neural graphs:
//! the ONNX export starts at phoneme ids, so the frontend has to reproduce the
//! same IPA that the model was trained on. That is eSpeak-NG's `en-us` voice in
//! IPA mode, which is the same frontend Piper and the wider VITS ecosystem use.
//!
//! Two details matter for matching the training-time frontend:
//!
//! * **Punctuation survives.** `espeak-ng --ipa -q` drops terminators and emits
//!   one line per clause, but VITS models are trained with `,`/`.`/`?`/`!` in the
//!   phoneme vocabulary because they carry prosody. [`phonemize`] therefore
//!   splits clauses itself and re-attaches each terminator after phonemizing.
//! * **Ids are per character, not per token.** IPA output is a string of Unicode
//!   scalars — including stress marks (`ˈ`, `ˌ`) and length (`ː`) — and each maps
//!   to its own id. See [`PhonemeVocab`].

use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

use anyhow::{bail, Context, Result};

/// eSpeak-NG voice the model's frontend was trained with.
const ESPEAK_VOICE: &str = "en-us";

/// Clause terminators kept as phonemes. Ordered longest-first so `...` is
/// consumed before `.`.
const TERMINATORS: [&str; 7] = ["...", ".", "!", "?", ";", ":", ","];

/// A clause plus the punctuation that ended it (empty for a trailing fragment).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Clause {
    pub text: String,
    pub terminator: String,
}

/// True when the `espeak-ng` binary is callable. The frontend shells out rather
/// than linking `libespeak-ng`, matching how the eSpeak engine in `engine.rs`
/// already invokes it and keeping the crate free of a C dependency.
pub fn espeak_available() -> bool {
    voxctrl_config::find_in_path("espeak-ng").is_some()
}

/// Split `text` into clauses on terminator punctuation, keeping the terminator.
///
/// `...` is treated as a single terminator so an ellipsis doesn't become three
/// separate pause phonemes.
pub fn split_clauses(text: &str) -> Vec<Clause> {
    let chars: Vec<char> = text.chars().collect();
    let mut clauses = Vec::new();
    let mut current = String::new();
    let mut i = 0;

    while i < chars.len() {
        let rest: String = chars[i..].iter().collect();
        let matched = TERMINATORS.iter().find(|t| rest.starts_with(**t));

        if let Some(term) = matched {
            let trimmed = current.trim();
            if !trimmed.is_empty() {
                clauses.push(Clause {
                    text: trimmed.to_string(),
                    terminator: (*term).to_string(),
                });
            }
            current.clear();
            i += term.chars().count();
        } else {
            current.push(chars[i]);
            i += 1;
        }
    }

    let trimmed = current.trim();
    if !trimmed.is_empty() {
        clauses.push(Clause {
            text: trimmed.to_string(),
            terminator: String::new(),
        });
    }

    clauses
}

/// Phonemize one clause with eSpeak-NG, returning bare IPA with no terminator.
///
/// eSpeak may still split a clause across lines (it breaks on some conjunctions);
/// those are rejoined with a space, which phonemizes to the same short pause.
fn phonemize_clause(text: &str) -> Result<String> {
    let output = Command::new("espeak-ng")
        .arg("-v")
        .arg(ESPEAK_VOICE)
        .arg("--ipa")
        .arg("-q")
        .arg("--")
        .arg(text)
        .output()
        .context("spawn espeak-ng for phonemization")?;

    if !output.status.success() {
        bail!(
            "espeak-ng phonemization failed with status {:?}: {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    let ipa = String::from_utf8_lossy(&output.stdout);
    Ok(ipa
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" "))
}

/// Convert `text` to the IPA phoneme string the model expects.
///
/// Each clause is phonemized in its own eSpeak invocation so its terminator can
/// be re-attached reliably — phonemizing the whole text at once would make the
/// clause-to-line mapping ambiguous whenever eSpeak introduces its own breaks.
pub fn phonemize(text: &str) -> Result<String> {
    if !espeak_available() {
        bail!(
            "espeak-ng is not installed on this system, and Inflect-Micro-v2 needs it \
             for grapheme-to-phoneme conversion. Install it with your package manager \
             (e.g. `sudo apt install espeak-ng` or `sudo pacman -S espeak-ng`)."
        );
    }

    let mut out = String::new();
    for clause in split_clauses(text) {
        let ipa = phonemize_clause(&clause.text)?;
        if ipa.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(&ipa);
        out.push_str(&clause.terminator);
    }

    Ok(out)
}

// ── Phoneme vocabulary ────────────────────────────────────────────────────────

/// Reserved symbol names used by VITS phoneme vocabularies.
const PAD: &str = "_";
const BOS: &str = "^";
const EOS: &str = "$";

/// Maps IPA characters to the model's phoneme ids.
///
/// The vocabulary ships with the ONNX export, so it is always read from disk when
/// present — see [`PhonemeVocab::load`] for the file names and formats accepted.
/// [`PhonemeVocab::conventional`] exists only as a documented fallback and is not
/// a substitute for the real table.
#[derive(Debug, Clone)]
pub struct PhonemeVocab {
    map: HashMap<String, i64>,
    /// Whether this table came from the model directory rather than the fallback.
    pub from_file: bool,
}

/// File names checked, in order, for the phoneme table inside the model dir.
pub const VOCAB_FILES: [&str; 4] = [
    "phonemes.json",
    "tokens.json",
    "vocab.json",
    "tokens.txt",
];

impl PhonemeVocab {
    /// Load the phoneme table from `dir`, trying each name in [`VOCAB_FILES`].
    ///
    /// Three on-disk shapes are accepted, covering how VITS exports usually ship
    /// their table:
    ///
    /// * `{"phoneme_id_map": {"a": [1], ...}}` — Piper's config layout
    /// * `{"a": 1, "b": 2, ...}` — a flat symbol→id object
    /// * `a 1\nb 2\n` — whitespace-separated `tokens.txt`
    ///
    /// Returns `Ok(None)` when no vocabulary file is present.
    pub fn load(dir: &Path) -> Result<Option<Self>> {
        for name in VOCAB_FILES {
            let path = dir.join(name);
            if !path.exists() {
                continue;
            }
            let raw = std::fs::read_to_string(&path)
                .with_context(|| format!("read phoneme vocabulary {}", path.display()))?;
            let map = if name.ends_with(".json") {
                parse_json_vocab(&raw)
                    .with_context(|| format!("parse phoneme vocabulary {}", path.display()))?
            } else {
                parse_text_vocab(&raw)
                    .with_context(|| format!("parse phoneme vocabulary {}", path.display()))?
            };
            if map.is_empty() {
                bail!("phoneme vocabulary {} is empty", path.display());
            }
            return Ok(Some(Self { map, from_file: true }));
        }
        Ok(None)
    }

    /// The conventional VITS phoneme table: pad/BOS/EOS at ids 0/1/2 followed by
    /// the IPA inventory eSpeak-NG emits for `en-us`.
    ///
    /// **This is a fallback, not the model's real table.** Ids assigned here are
    /// almost certainly not Inflect's, so audio produced through it would be
    /// wrong. [`super::model::InflectModel`] only reaches for this when the model
    /// directory ships no vocabulary file, and refuses to synthesize with it
    /// unless explicitly permitted.
    pub fn conventional() -> Self {
        let mut map = HashMap::new();
        for (i, sym) in [PAD, BOS, EOS].iter().enumerate() {
            map.insert((*sym).to_string(), i as i64);
        }
        // eSpeak-NG `en-us` IPA inventory plus the punctuation VITS keeps.
        const SYMBOLS: &str = " !\",-.:;?abdefhijklmnopqrstuvwxyzæðŋɐɑɔəɚɛɜɡɪɫɬɹɾʃʊʌʒʔθˈˌːᵻ‍";
        for (i, c) in SYMBOLS.chars().enumerate() {
            map.insert(c.to_string(), (i + 3) as i64);
        }
        Self { map, from_file: false }
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    fn id(&self, symbol: &str) -> Option<i64> {
        self.map.get(symbol).copied()
    }

    /// Encode an IPA string into model input ids.
    ///
    /// Follows the VITS convention the architecture implies: a BOS, then every
    /// phoneme separated by a pad ("blank") symbol, then an EOS. The interleaved
    /// pad is what the monotonic alignment attends over, so dropping it would
    /// change the alignment length the duration predictor sees.
    ///
    /// Characters absent from the vocabulary are skipped and reported in the
    /// returned `skipped` list rather than failing the utterance — an unknown
    /// symbol should cost one phoneme, not the whole sentence.
    pub fn encode(&self, ipa: &str) -> Encoded {
        let pad = self.id(PAD);
        let mut ids = Vec::new();
        let mut skipped = Vec::new();

        if let Some(bos) = self.id(BOS) {
            ids.push(bos);
        }
        if let Some(p) = pad {
            ids.push(p);
        }

        for c in ipa.chars() {
            let sym = c.to_string();
            match self.id(&sym) {
                Some(id) => {
                    ids.push(id);
                    if let Some(p) = pad {
                        ids.push(p);
                    }
                }
                None => {
                    if !skipped.contains(&sym) {
                        skipped.push(sym);
                    }
                }
            }
        }

        if let Some(eos) = self.id(EOS) {
            ids.push(eos);
        }

        Encoded { ids, skipped }
    }
}

/// Phoneme ids plus any symbols that had no entry in the vocabulary.
#[derive(Debug, Clone)]
pub struct Encoded {
    pub ids: Vec<i64>,
    pub skipped: Vec<String>,
}

fn parse_json_vocab(raw: &str) -> Result<HashMap<String, i64>> {
    let value: serde_json::Value = serde_json::from_str(raw).context("invalid JSON")?;

    // Piper-style config: the table lives under `phoneme_id_map`.
    let table = value
        .get("phoneme_id_map")
        .or_else(|| value.get("vocab"))
        .unwrap_or(&value);

    let obj = table
        .as_object()
        .context("expected a JSON object mapping phonemes to ids")?;

    let mut map = HashMap::with_capacity(obj.len());
    for (symbol, id) in obj {
        // Values are either a bare id or a single-element array (Piper's shape).
        let resolved = match id {
            serde_json::Value::Number(n) => n.as_i64(),
            serde_json::Value::Array(a) => a.first().and_then(|v| v.as_i64()),
            _ => None,
        };
        if let Some(v) = resolved {
            map.insert(symbol.clone(), v);
        }
    }
    Ok(map)
}

fn parse_text_vocab(raw: &str) -> Result<HashMap<String, i64>> {
    let mut map = HashMap::new();
    for (line_no, line) in raw.lines().enumerate() {
        let line = line.trim_end_matches(['\r', '\n']);
        if line.trim().is_empty() {
            continue;
        }
        // `<symbol> <id>`; the symbol may itself be a space, so split from the right.
        let Some((symbol, id)) = line.rsplit_once(char::is_whitespace) else {
            bail!("line {} is not `<symbol> <id>`: {line:?}", line_no + 1);
        };
        let id: i64 = id
            .trim()
            .parse()
            .with_context(|| format!("line {} has a non-numeric id", line_no + 1))?;
        map.insert(symbol.to_string(), id);
    }
    Ok(map)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    // ── Clause splitting ─────────────────────────────────────────────────────

    #[test]
    fn test_split_clauses_keeps_terminators() {
        let clauses = split_clauses("Hello world. How are you?");
        assert_eq!(clauses.len(), 2);
        assert_eq!(clauses[0].text, "Hello world");
        assert_eq!(clauses[0].terminator, ".");
        assert_eq!(clauses[1].text, "How are you");
        assert_eq!(clauses[1].terminator, "?");
    }

    #[test]
    fn test_split_clauses_ellipsis_is_one_terminator() {
        let clauses = split_clauses("Wait... really");
        assert_eq!(clauses.len(), 2);
        assert_eq!(clauses[0].terminator, "...");
    }

    #[test]
    fn test_split_clauses_trailing_fragment_has_empty_terminator() {
        let clauses = split_clauses("no punctuation here");
        assert_eq!(clauses.len(), 1);
        assert_eq!(clauses[0].terminator, "");
    }

    #[test]
    fn test_split_clauses_ignores_empty_segments() {
        // Repeated punctuation must not produce empty clauses.
        let clauses = split_clauses("Hi!!! There");
        assert_eq!(clauses.len(), 2);
        assert_eq!(clauses[0].text, "Hi");
        assert_eq!(clauses[1].text, "There");
    }

    #[test]
    fn test_split_clauses_empty_input() {
        assert!(split_clauses("").is_empty());
        assert!(split_clauses("   ").is_empty());
    }

    #[test]
    fn test_split_clauses_commas_are_clause_boundaries() {
        let clauses = split_clauses("one, two, three.");
        assert_eq!(clauses.len(), 3);
        assert_eq!(clauses[0].terminator, ",");
        assert_eq!(clauses[2].terminator, ".");
    }

    // ── Vocabulary parsing ───────────────────────────────────────────────────

    #[test]
    fn test_parse_json_vocab_flat_object() {
        let map = parse_json_vocab(r#"{"_": 0, "a": 1, "b": 2}"#).unwrap();
        assert_eq!(map.get("a"), Some(&1));
        assert_eq!(map.get("b"), Some(&2));
    }

    #[test]
    fn test_parse_json_vocab_piper_phoneme_id_map() {
        let map = parse_json_vocab(r#"{"phoneme_id_map": {"_": [0], "a": [5]}}"#).unwrap();
        assert_eq!(map.get("_"), Some(&0));
        assert_eq!(map.get("a"), Some(&5));
    }

    #[test]
    fn test_parse_text_vocab_symbol_id_pairs() {
        let map = parse_text_vocab("_ 0\na 1\nb 2\n").unwrap();
        assert_eq!(map.len(), 3);
        assert_eq!(map.get("b"), Some(&2));
    }

    #[test]
    fn test_parse_text_vocab_handles_space_symbol() {
        // The symbol itself can be a space, so parsing must split from the right.
        let map = parse_text_vocab("  4\n").unwrap();
        assert_eq!(map.get(" "), Some(&4));
    }

    #[test]
    fn test_parse_text_vocab_rejects_non_numeric_id() {
        assert!(parse_text_vocab("a notanumber\n").is_err());
    }

    // ── Vocabulary loading ───────────────────────────────────────────────────

    #[test]
    fn test_vocab_load_returns_none_when_absent() {
        let dir = tempdir().unwrap();
        assert!(PhonemeVocab::load(dir.path()).unwrap().is_none());
    }

    #[test]
    fn test_vocab_load_reads_phonemes_json() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("phonemes.json"), r#"{"_":0,"^":1,"$":2,"a":3}"#).unwrap();
        let vocab = PhonemeVocab::load(dir.path()).unwrap().unwrap();
        assert!(vocab.from_file);
        assert_eq!(vocab.len(), 4);
    }

    #[test]
    fn test_vocab_load_reads_tokens_txt() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("tokens.txt"), "_ 0\n^ 1\n$ 2\na 3\n").unwrap();
        let vocab = PhonemeVocab::load(dir.path()).unwrap().unwrap();
        assert!(vocab.from_file);
        assert_eq!(vocab.id("a"), Some(3));
    }

    #[test]
    fn test_vocab_load_rejects_empty_file() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("phonemes.json"), "{}").unwrap();
        assert!(PhonemeVocab::load(dir.path()).is_err());
    }

    // ── Encoding ─────────────────────────────────────────────────────────────

    #[test]
    fn test_encode_interleaves_pad_and_wraps_with_bos_eos() {
        let vocab = PhonemeVocab::conventional();
        let encoded = vocab.encode("ab");
        // ^ _ a _ b _ $
        assert_eq!(encoded.ids.first(), Some(&1), "starts with BOS");
        assert_eq!(encoded.ids.last(), Some(&2), "ends with EOS");
        assert_eq!(encoded.ids[1], 0, "pad follows BOS");
        assert_eq!(encoded.ids.len(), 7);
        assert!(encoded.skipped.is_empty());
    }

    #[test]
    fn test_encode_reports_unknown_symbols_without_failing() {
        let vocab = PhonemeVocab::conventional();
        // `%` is not a phoneme and is not in the inventory.
        let encoded = vocab.encode("a%b");
        assert_eq!(encoded.skipped, vec!["%".to_string()]);
        assert_eq!(encoded.ids.len(), 7, "unknown symbol contributes no ids");
    }

    #[test]
    fn test_encode_dedupes_skipped_symbols() {
        let vocab = PhonemeVocab::conventional();
        let encoded = vocab.encode("%%%");
        assert_eq!(encoded.skipped.len(), 1);
    }

    #[test]
    fn test_encode_empty_input_is_bos_pad_eos() {
        let vocab = PhonemeVocab::conventional();
        let encoded = vocab.encode("");
        assert_eq!(encoded.ids, vec![1, 0, 2]);
    }

    #[test]
    fn test_conventional_vocab_marks_itself_as_not_from_file() {
        assert!(!PhonemeVocab::conventional().from_file);
    }

    #[test]
    fn test_conventional_vocab_covers_common_espeak_ipa() {
        let vocab = PhonemeVocab::conventional();
        // Stress and length marks are real phonemes in eSpeak IPA output.
        for sym in ["ˈ", "ː", "ɹ", "ə", "ŋ", " ", ".", ","] {
            assert!(vocab.id(sym).is_some(), "missing {sym:?} from inventory");
        }
    }

    // ── eSpeak-backed phonemization ──────────────────────────────────────────
    //
    // Gated on the binary being installed so the suite still passes on machines
    // without eSpeak-NG.

    #[test]
    fn test_phonemize_produces_ipa_when_espeak_present() {
        if !espeak_available() {
            return;
        }
        let ipa = phonemize("Hello world.").unwrap();
        assert!(!ipa.is_empty());
        // eSpeak's en-us rendering of "hello world" contains a primary stress mark.
        assert!(ipa.contains('ˈ'), "expected stress mark in {ipa:?}");
        assert!(ipa.ends_with('.'), "terminator must survive: {ipa:?}");
    }

    #[test]
    fn test_phonemize_encodes_through_conventional_vocab() {
        if !espeak_available() {
            return;
        }
        let ipa = phonemize("Testing one two three.").unwrap();
        let encoded = PhonemeVocab::conventional().encode(&ipa);
        assert!(encoded.ids.len() > 10);
    }

    #[test]
    fn test_phonemize_empty_text_is_empty() {
        if !espeak_available() {
            return;
        }
        assert!(phonemize("").unwrap().is_empty());
    }
}

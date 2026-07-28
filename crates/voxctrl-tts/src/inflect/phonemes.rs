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

/// Maps IPA characters to the model's phoneme ids.
///
/// Ids come from the symbol's position in the export's ordered `symbols` list,
/// so the list itself is the vocabulary — see [`PhonemeVocab::load`] for the
/// file names and formats accepted.
#[derive(Debug, Clone)]
pub struct PhonemeVocab {
    map: HashMap<String, i64>,
    /// Whether this table came from the model directory rather than the fallback.
    pub from_file: bool,
    /// Which file it was read from, for logs and diagnostics.
    pub source: Option<std::path::PathBuf>,
}

/// File names checked first for the phoneme table. Not exhaustive — [`PhonemeVocab::load`]
/// falls back to scanning every text file in the directory.
pub const VOCAB_FILES: [&str; 6] = [
    // The export derives ids from the ordered list in `text/symbols.py`.
    "symbols.py",
    "symbols.json",
    "phonemes.json",
    "tokens.json",
    "vocab.json",
    "tokens.txt",
];

/// Extensions considered when scanning for an unnamed phoneme table.
const VOCAB_EXTENSIONS: [&str; 5] = ["json", "txt", "csv", "tsv", "py"];

/// A phoneme table has to be big enough to cover an IPA inventory and made
/// mostly of short symbols. This is what separates a real table from a
/// `config.json` of hyperparameters, whose keys are long words.
const MIN_VOCAB_ENTRIES: usize = 16;

fn is_plausible_vocab(map: &HashMap<String, i64>) -> bool {
    if map.len() < MIN_VOCAB_ENTRIES {
        return false;
    }
    let short = map.keys().filter(|k| k.chars().count() <= 3).count();
    short * 2 >= map.len()
}

/// Well-known names first, then any other text file in the directory, so a table
/// under an unexpected name is still found.
fn candidate_vocab_paths(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut paths: Vec<std::path::PathBuf> = VOCAB_FILES
        .iter()
        .map(|n| dir.join(n))
        .filter(|p| p.exists())
        .collect();

    let Ok(entries) = std::fs::read_dir(dir) else { return paths };
    let mut extra: Vec<std::path::PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && p.extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|e| VOCAB_EXTENSIONS.iter().any(|v| e.eq_ignore_ascii_case(v)))
                && !paths.contains(p)
        })
        .collect();
    // Deterministic order so the chosen table doesn't depend on directory iteration.
    extra.sort();
    paths.append(&mut extra);
    paths
}

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
    /// Returns `Ok(None)` when no file in `dir` parses as a phoneme table.
    ///
    /// The export's table is not reliably named, so rather than requiring a
    /// specific filename this tries the well-known names first and then every
    /// other text file in the directory, accepting the first whose contents
    /// actually look like a phoneme table (see [`is_plausible_vocab`]). That
    /// keeps a `config.json` full of model hyperparameters from being mistaken
    /// for the vocabulary.
    pub fn load(dir: &Path) -> Result<Option<Self>> {
        for path in candidate_vocab_paths(dir) {
            let Ok(raw) = std::fs::read_to_string(&path) else { continue };
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or_default()
                .to_ascii_lowercase();
            let parsed = match ext.as_str() {
                "json" => parse_json_vocab(&raw),
                "py" => parse_python_symbols(&raw).map(map_from_ordered),
                _ => parse_text_vocab(&raw),
            };
            let Ok(map) = parsed else { continue };
            if !is_plausible_vocab(&map) {
                continue;
            }
            return Ok(Some(Self {
                map,
                from_file: true,
                source: Some(path),
            }));
        }
        Ok(None)
    }

    /// Build a table from an ordered symbol list, where a symbol's id is its
    /// position — the same rule the export uses (`{symbol: index for index,
    /// symbol in enumerate(symbols)}`).
    pub fn from_symbols<I, S>(symbols: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let map = symbols
            .into_iter()
            .enumerate()
            .map(|(i, s)| (s.into(), i as i64))
            .collect();
        Self { map, from_file: false, source: None }
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    fn id(&self, symbol: &str) -> Option<i64> {
        self.map.get(symbol).copied()
    }

    /// Encode an IPA string into model input ids.
    ///
    /// Mirrors `phonemes_to_tokens` in the export's reference script exactly:
    /// each symbol maps to its index in the ordered symbol list, and the result
    /// is interleaved with blanks into `[0, s₀, 0, s₁, …, sₙ₋₁, 0]` — length
    /// `2n+1`. The blank is the literal id `0`, and there is **no** BOS/EOS
    /// wrapper; the interleaved blanks are what the monotonic alignment attends
    /// over, so the length has to come out exactly right.
    ///
    /// Characters absent from the vocabulary are skipped and reported in the
    /// returned `skipped` list rather than failing the utterance — an unknown
    /// symbol should cost one phoneme, not the whole sentence.
    pub fn encode(&self, ipa: &str) -> Encoded {
        let mut sequence = Vec::new();
        let mut skipped = Vec::new();

        for c in ipa.chars() {
            let sym = c.to_string();
            match self.id(&sym) {
                Some(id) => sequence.push(id),
                None => {
                    if !skipped.contains(&sym) {
                        skipped.push(sym);
                    }
                }
            }
        }

        if sequence.is_empty() {
            return Encoded { ids: Vec::new(), skipped };
        }

        let mut ids = vec![0i64; sequence.len() * 2 + 1];
        for (i, id) in sequence.iter().enumerate() {
            ids[i * 2 + 1] = *id;
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

/// Parse an ordered symbol list out of a Python source file.
///
/// The export defines its inventory in `text/symbols.py` the way the tacotron
/// lineage does — named string constants combined into one list:
///
/// ```text
/// _pad = '_'
/// _punctuation = ';:,.!?… '
/// symbols = [_pad] + list(_punctuation) + list(_letters) + list(_letters_ipa)
/// ```
///
/// so this resolves the constants and expands the concatenation. `[name]`
/// contributes the constant as a single symbol; `list(name)` contributes one
/// symbol per character. A plain list literal (`symbols = ['a', 'b']`) is also
/// accepted, since other exports write it that way. Nothing is executed.
fn parse_python_symbols(raw: &str) -> Result<Vec<String>> {
    let constants = python_string_constants(raw);

    let expr = raw
        .lines()
        .map(str::trim)
        .find_map(|line| {
            let rest = line.strip_prefix("symbols")?.trim_start();
            // Guards against `symbols_other = ...` and `symbols.index(...)`.
            rest.strip_prefix('=').map(str::trim)
        })
        .context("no `symbols = ...` assignment found")?;

    let mut out = Vec::new();
    for term in split_top_level(expr, '+') {
        expand_symbol_term(&term, &constants, &mut out)
            .with_context(|| format!("in term {term:?}"))?;
    }
    if out.is_empty() {
        bail!("`symbols` resolved to an empty list");
    }
    Ok(out)
}

/// Collect every `NAME = "..."` string constant in the file.
fn python_string_constants(raw: &str) -> HashMap<String, String> {
    let mut constants = HashMap::new();
    for line in raw.lines() {
        let Some((lhs, rhs)) = line.split_once('=') else { continue };
        let name = lhs.trim();
        if name.is_empty() || name.contains(char::is_whitespace) {
            continue;
        }
        let rhs = rhs.trim();
        if !rhs.starts_with('\'') && !rhs.starts_with('"') {
            continue;
        }
        if let Ok(value) = unquote_python_string(rhs) {
            constants.insert(name.to_string(), value);
        }
    }
    constants
}

/// Expand one `+`-separated term of the `symbols` expression.
fn expand_symbol_term(
    term: &str,
    constants: &HashMap<String, String>,
    out: &mut Vec<String>,
) -> Result<()> {
    let term = term.trim();

    // `list(X)` — one symbol per character of X.
    if let Some(inner) = term.strip_prefix("list(").and_then(|t| t.strip_suffix(')')) {
        let value = resolve_string(inner.trim(), constants)?;
        out.extend(value.chars().map(|c| c.to_string()));
        return Ok(());
    }

    // `[...]` — either a single wrapped constant, or a literal list.
    if let Some(inner) = term.strip_prefix('[').and_then(|t| t.strip_suffix(']')) {
        for item in split_top_level(inner, ',') {
            out.push(resolve_string(item.trim(), constants)?);
        }
        return Ok(());
    }

    bail!("unsupported term (expected `[...]` or `list(...)`)")
}

/// Resolve a token that is either a string literal or the name of one.
fn resolve_string(token: &str, constants: &HashMap<String, String>) -> Result<String> {
    if token.starts_with('\'') || token.starts_with('"') {
        return unquote_python_string(token);
    }
    constants
        .get(token)
        .cloned()
        .with_context(|| format!("unknown name {token:?}"))
}

/// Split on `separator` at bracket depth zero and outside quotes.
///
/// Quote tracking matters here: the inventory's punctuation constant contains a
/// `"` inside single quotes, and the IPA constant contains `'` inside double
/// quotes, so a naive split would tear those strings apart.
fn split_top_level(text: &str, separator: char) -> Vec<String> {
    let mut items = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    let mut depth = 0usize;
    let mut escaped = false;

    for c in text.chars() {
        match quote {
            Some(q) => {
                current.push(c);
                if escaped {
                    escaped = false;
                } else if c == '\\' {
                    escaped = true;
                } else if c == q {
                    quote = None;
                }
            }
            None => match c {
                '\'' | '"' => {
                    quote = Some(c);
                    current.push(c);
                }
                '(' | '[' => {
                    depth += 1;
                    current.push(c);
                }
                ')' | ']' => {
                    depth = depth.saturating_sub(1);
                    current.push(c);
                }
                // A comment ends the expression.
                '#' if depth == 0 => break,
                c if c == separator && depth == 0 => {
                    if !current.trim().is_empty() {
                        items.push(current.trim().to_string());
                    }
                    current.clear();
                }
                _ => current.push(c),
            },
        }
    }
    if !current.trim().is_empty() {
        items.push(current.trim().to_string());
    }
    items
}

/// Strip the quotes from a Python string literal and resolve its escapes.
fn unquote_python_string(token: &str) -> Result<String> {
    let token = token.trim();
    let mut chars = token.chars();
    let open = chars.next().context("empty list item")?;
    if open != '\'' && open != '"' {
        bail!("list item {token:?} is not a string literal");
    }
    let body = token
        .strip_prefix(open)
        .and_then(|s| s.strip_suffix(open))
        .with_context(|| format!("unterminated string literal {token:?}"))?;

    let mut out = String::new();
    let mut chars = body.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('u') => {
                // \uXXXX — IPA symbols are commonly written this way.
                let hex: String = chars.by_ref().take(4).collect();
                let code = u32::from_str_radix(&hex, 16)
                    .with_context(|| format!("bad \\u escape in {token:?}"))?;
                out.push(char::from_u32(code).context("invalid code point")?);
            }
            Some(other) => out.push(other),
            None => bail!("trailing backslash in {token:?}"),
        }
    }
    Ok(out)
}

/// Turn an ordered symbol list into a symbol→id map, where id is the position.
fn map_from_ordered(symbols: Vec<String>) -> HashMap<String, i64> {
    symbols
        .into_iter()
        .enumerate()
        .map(|(i, s)| (s, i as i64))
        .collect()
}

fn parse_json_vocab(raw: &str) -> Result<HashMap<String, i64>> {
    let value: serde_json::Value = serde_json::from_str(raw).context("invalid JSON")?;

    // An ordered array is the same shape as `text/symbols.py`: id is position.
    if let Some(array) = value.as_array() {
        let symbols: Vec<String> = array
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect();
        if symbols.len() == array.len() && !symbols.is_empty() {
            return Ok(map_from_ordered(symbols));
        }
    }

    // Piper-style config: the table lives under `phoneme_id_map`.
    let table = value
        .get("phoneme_id_map")
        .or_else(|| value.get("vocab"))
        .or_else(|| value.get("symbols"))
        .unwrap_or(&value);

    if let Some(array) = table.as_array() {
        let symbols: Vec<String> = array
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect();
        if symbols.len() == array.len() && !symbols.is_empty() {
            return Ok(map_from_ordered(symbols));
        }
    }

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

    // ── Python symbol-list parsing ───────────────────────────────────────────
    //
    // The export's text/symbols.py builds its inventory from named constants
    // rather than a literal list, in the tacotron style.

    const TACOTRON_SYMBOLS_PY: &str = r#"
_pad        = '_'
_punctuation = ';:,.!?\u00a1\u00bf\u2014\u2026"\u00ab\u00bb\u201c\u201d '
_letters = 'ABC'
_letters_ipa = "\u0251\u0250'\u0329'\u1d7b"

# Export all symbols:
symbols = [_pad] + list(_punctuation) + list(_letters) + list(_letters_ipa)

# Special symbol ids
SPACE_ID = symbols.index(" ")
"#;

    #[test]
    fn test_parse_python_symbols_expands_constant_concatenation() {
        let symbols = parse_python_symbols(TACOTRON_SYMBOLS_PY).unwrap();
        assert_eq!(symbols[0], "_", "the pad constant comes first, as one symbol");
        assert_eq!(symbols[1], ";");
        // 1 pad + 16 punctuation + 3 letters + 6 ipa (the ipa fixture is
        // ɑ, ɐ, ', U+0329, ', ᵻ — the quotes are literal characters).
        assert_eq!(symbols.len(), 1 + 16 + 3 + 6);
    }

    #[test]
    fn test_parse_python_symbols_ids_match_position() {
        let map = map_from_ordered(parse_python_symbols(TACOTRON_SYMBOLS_PY).unwrap());
        assert_eq!(map.get("_"), Some(&0), "pad is id 0");
        // The space is the last of the 16 punctuation characters.
        assert_eq!(map.get(" "), Some(&16), "matches the export's SPACE_ID");
    }

    #[test]
    fn test_parse_python_symbols_keeps_quotes_inside_strings() {
        // _punctuation is single-quoted but contains a double quote, and
        // _letters_ipa is double-quoted but contains single quotes. A splitter
        // that ignored quoting would tear both apart.
        let symbols = parse_python_symbols(TACOTRON_SYMBOLS_PY).unwrap();
        assert!(symbols.contains(&"\"".to_string()), "kept the double quote");
        assert!(symbols.contains(&"'".to_string()), "kept the single quotes");
    }

    #[test]
    fn test_parse_python_symbols_duplicate_takes_last_id() {
        // The real inventory lists `'` twice; Python's dict comprehension keeps
        // the later index, and so must this.
        let symbols = parse_python_symbols(TACOTRON_SYMBOLS_PY).unwrap();
        let positions: Vec<usize> = symbols
            .iter()
            .enumerate()
            .filter(|(_, s)| s.as_str() == "'")
            .map(|(i, _)| i)
            .collect();
        assert_eq!(positions.len(), 2, "the fixture has the duplicate");
        let map = map_from_ordered(symbols);
        assert_eq!(map.get("'"), Some(&(*positions.last().unwrap() as i64)));
    }

    #[test]
    fn test_parse_python_symbols_accepts_a_plain_list_literal() {
        let symbols = parse_python_symbols("symbols = ['a', 'b', 'c']").unwrap();
        assert_eq!(symbols, vec!["a", "b", "c"]);
    }

    #[test]
    fn test_parse_python_symbols_ignores_the_symbols_index_line() {
        // `SPACE_ID = symbols.index(" ")` must not be mistaken for the assignment.
        let symbols = parse_python_symbols("symbols = ['a']\nSPACE_ID = symbols.index(\" \")\n").unwrap();
        assert_eq!(symbols, vec!["a"]);
    }

    #[test]
    fn test_parse_python_symbols_rejects_missing_assignment() {
        assert!(parse_python_symbols("_pad = '_'\n").is_err());
    }

    #[test]
    fn test_parse_python_symbols_rejects_unknown_name() {
        assert!(parse_python_symbols("symbols = list(_never_defined)").is_err());
    }

    #[test]
    fn test_split_top_level_respects_quotes_and_depth() {
        let parts = split_top_level("[_a] + list(_b) + list(_c)", '+');
        assert_eq!(parts, vec!["[_a]", "list(_b)", "list(_c)"]);
        // A separator inside quotes is not a separator.
        assert_eq!(split_top_level("'a+b' + _c", '+'), vec!["'a+b'", "_c"]);
    }

    #[test]
    fn test_unquote_python_string_resolves_unicode_escapes() {
        assert_eq!(unquote_python_string(r"'\u0251'").unwrap(), "ɑ");
        assert_eq!(unquote_python_string("\"a'b\"").unwrap(), "a'b");
    }

    #[test]
    fn test_vocab_load_reads_symbols_py() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("symbols.py"), TACOTRON_SYMBOLS_PY).unwrap();
        let vocab = PhonemeVocab::load(dir.path()).unwrap().unwrap();
        assert_eq!(vocab.id("_"), Some(0));
        assert!(vocab.source.unwrap().ends_with("symbols.py"));
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

    /// A table with enough short symbols to pass the plausibility check.
    fn vocab_text() -> String {
        "_^$abdefhijklmnopqrstuvwxyzəɪˈː"
            .chars()
            .enumerate()
            .map(|(i, c)| format!("{c} {i}\n"))
            .collect()
    }

    #[test]
    fn test_vocab_load_reads_phonemes_json() {
        let dir = tempdir().unwrap();
        let entries: Vec<String> = vocab_text()
            .lines()
            .map(|l| {
                let (s, i) = l.rsplit_once(' ').unwrap();
                format!("{}: {i}", serde_json::to_string(s).unwrap())
            })
            .collect();
        std::fs::write(
            dir.path().join("phonemes.json"),
            format!("{{{}}}", entries.join(",")),
        )
        .unwrap();
        let vocab = PhonemeVocab::load(dir.path()).unwrap().unwrap();
        assert!(vocab.from_file);
        assert_eq!(vocab.id("a"), Some(3));
    }

    #[test]
    fn test_vocab_load_reads_tokens_txt() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("tokens.txt"), vocab_text()).unwrap();
        let vocab = PhonemeVocab::load(dir.path()).unwrap().unwrap();
        assert!(vocab.from_file);
        assert_eq!(vocab.id("a"), Some(3));
    }

    #[test]
    fn test_vocab_load_finds_table_under_unexpected_name() {
        // The published export does not use any of the well-known names, so the
        // table has to be found by parsing rather than by filename.
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("inflect_symbols.txt"), vocab_text()).unwrap();
        let vocab = PhonemeVocab::load(dir.path()).unwrap().unwrap();
        assert_eq!(vocab.id("a"), Some(3));
        assert!(vocab.source.unwrap().ends_with("inflect_symbols.txt"));
    }

    #[test]
    fn test_vocab_load_prefers_well_known_name_over_scanned_file() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("aaa_other.txt"), vocab_text()).unwrap();
        std::fs::write(dir.path().join("tokens.txt"), vocab_text()).unwrap();
        let vocab = PhonemeVocab::load(dir.path()).unwrap().unwrap();
        assert!(vocab.source.unwrap().ends_with("tokens.txt"));
    }

    #[test]
    fn test_vocab_load_skips_hyperparameter_config() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("config.json"),
            r#"{"sample_rate":24000,"hidden_channels":192,"n_layers":6,"filter_channels":768}"#,
        )
        .unwrap();
        assert!(
            PhonemeVocab::load(dir.path()).unwrap().is_none(),
            "long hyperparameter keys must not look like a phoneme table"
        );
    }

    #[test]
    fn test_vocab_load_skips_empty_and_unparseable_files() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("phonemes.json"), "{}").unwrap();
        std::fs::write(dir.path().join("notes.txt"), "just some prose, not a table").unwrap();
        assert!(PhonemeVocab::load(dir.path()).unwrap().is_none());
    }

    #[test]
    fn test_vocab_load_recovers_when_first_candidate_is_junk() {
        // An unparseable well-known name must not stop a valid table elsewhere
        // from being found.
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("phonemes.json"), "not json at all").unwrap();
        std::fs::write(dir.path().join("symbols.txt"), vocab_text()).unwrap();
        let vocab = PhonemeVocab::load(dir.path()).unwrap().unwrap();
        assert!(vocab.source.unwrap().ends_with("symbols.txt"));
    }

    // ── Plausibility heuristic ───────────────────────────────────────────────

    #[test]
    fn test_is_plausible_vocab_rejects_small_tables() {
        let map: HashMap<String, i64> =
            (0..4).map(|i| (i.to_string(), i as i64)).collect();
        assert!(!is_plausible_vocab(&map));
    }

    #[test]
    fn test_is_plausible_vocab_rejects_long_keys() {
        let map: HashMap<String, i64> = (0..40)
            .map(|i| (format!("some_long_config_key_{i}"), i as i64))
            .collect();
        assert!(!is_plausible_vocab(&map));
    }

    #[test]
    fn test_is_plausible_vocab_accepts_short_symbol_table() {
        let map: HashMap<String, i64> = "_^$abdefhijklmnopqrstuvwxyz"
            .chars()
            .enumerate()
            .map(|(i, c)| (c.to_string(), i as i64))
            .collect();
        assert!(is_plausible_vocab(&map));
    }

    // ── Encoding ─────────────────────────────────────────────────────────────

    /// A small ordered inventory: ids are positions, so `_`=0, `a`=1, `b`=2.
    fn test_vocab() -> PhonemeVocab {
        PhonemeVocab::from_symbols(["_", "a", "b", "c", "ˈ", "ː", " ", "."])
    }

    #[test]
    fn test_encode_interleaves_blanks_without_bos_or_eos() {
        // Reference: with_blanks[1::2] = sequence, length 2n+1, blank id 0.
        let encoded = test_vocab().encode("ab");
        assert_eq!(encoded.ids, vec![0, 1, 0, 2, 0]);
        assert!(encoded.skipped.is_empty());
    }

    #[test]
    fn test_encode_length_is_two_n_plus_one() {
        let encoded = test_vocab().encode("abc");
        assert_eq!(encoded.ids.len(), 3 * 2 + 1);
        assert_eq!(encoded.ids[0], 0, "starts with a blank");
        assert_eq!(*encoded.ids.last().unwrap(), 0, "ends with a blank");
    }

    #[test]
    fn test_encode_reports_unknown_symbols_without_failing() {
        // `%` is not in the inventory; it contributes no id and no blank.
        let encoded = test_vocab().encode("a%b");
        assert_eq!(encoded.skipped, vec!["%".to_string()]);
        assert_eq!(encoded.ids, vec![0, 1, 0, 2, 0]);
    }

    #[test]
    fn test_encode_dedupes_skipped_symbols() {
        let encoded = test_vocab().encode("%%%");
        assert_eq!(encoded.skipped.len(), 1);
    }

    #[test]
    fn test_encode_empty_input_yields_no_ids() {
        // The reference raises on an empty sequence; returning empty lets the
        // caller skip the chunk instead of failing the whole utterance.
        assert!(test_vocab().encode("").ids.is_empty());
        assert!(test_vocab().encode("%%%").ids.is_empty());
    }

    #[test]
    fn test_from_symbols_assigns_ids_by_position() {
        let v = PhonemeVocab::from_symbols(["_", "x", "y"]);
        assert_eq!(v.id("_"), Some(0));
        assert_eq!(v.id("x"), Some(1));
        assert_eq!(v.id("y"), Some(2));
    }

    #[test]
    fn test_from_symbols_is_not_marked_as_from_file() {
        assert!(!PhonemeVocab::from_symbols(["_"]).from_file);
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
    fn test_phonemize_encodes_through_a_symbol_table() {
        if !espeak_available() {
            return;
        }
        let ipa = phonemize("Testing one two three.").unwrap();
        let encoded = test_vocab().encode(&ipa);
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



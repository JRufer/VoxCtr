//! The timestamp prefix the `file` delivery target writes ahead of each line.
//!
//! The pattern is a chrono `strftime` string, so the same text both validates
//! and renders — the Settings UI previews a format through [`render_timestamp`]
//! and shows what the file will actually receive, rather than guessing at
//! chrono's specifier set in TypeScript.

use chrono::format::{Item, StrftimeItems};
use chrono::{DateTime, Utc};

/// The format used before the pattern was configurable, and the fallback for
/// an empty or unusable one.
pub const DEFAULT_TIMESTAMP_FORMAT: &str = "%Y-%m-%dT%H:%M:%SZ";

pub fn default_file_timestamp_format() -> String {
    DEFAULT_TIMESTAMP_FORMAT.to_string()
}

/// Check a strftime pattern without rendering it.
///
/// chrono only reports a bad specifier when the format is *displayed*, and it
/// does so by panicking, so this walks the parsed items instead and reports the
/// problem as an error the UI can show.
pub fn validate_timestamp_format(fmt: &str) -> Result<(), String> {
    if fmt.trim().is_empty() {
        return Err("Timestamp format cannot be empty.".into());
    }
    if StrftimeItems::new(fmt).any(|item| item == Item::Error) {
        return Err(
            "Not a valid timestamp format — check the % specifiers (e.g. %Y, %m, %d, %H).".into(),
        );
    }
    Ok(())
}

/// Render `at` with `fmt`, or explain why the pattern cannot be used.
pub fn render_timestamp(fmt: &str, at: DateTime<Utc>) -> Result<String, String> {
    validate_timestamp_format(fmt)?;

    let items: Vec<Item> = StrftimeItems::new(fmt).collect();
    Ok(at.format_with_items(items.iter()).to_string())
}

/// Render the prefix the file target writes now, falling back to
/// [`DEFAULT_TIMESTAMP_FORMAT`] when the configured pattern is unusable.
///
/// A bad pattern must never cost the user their dictation, so this degrades to
/// the default format with a warning instead of failing the delivery.
pub fn format_now(fmt: &str) -> String {
    let now = Utc::now();
    match render_timestamp(fmt, now) {
        Ok(rendered) => rendered,
        Err(e) => {
            tracing::warn!("Unusable file timestamp format {fmt:?} ({e}); using the default");
            render_timestamp(DEFAULT_TIMESTAMP_FORMAT, now)
                .unwrap_or_else(|_| now.to_rfc3339())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn fixed() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, 4, 17, 5, 9).unwrap()
    }

    #[test]
    fn renders_the_default_format() {
        assert_eq!(
            render_timestamp(DEFAULT_TIMESTAMP_FORMAT, fixed()).unwrap(),
            "2026-09-04T17:05:09Z"
        );
    }

    #[test]
    fn renders_custom_patterns() {
        assert_eq!(
            render_timestamp("%d/%m/%Y %H:%M", fixed()).unwrap(),
            "04/09/2026 17:05"
        );
        assert_eq!(
            render_timestamp("%A, %B %-d %Y", fixed()).unwrap(),
            "Friday, September 4 2026"
        );
        // Literal text around the specifiers is kept as written.
        assert_eq!(
            render_timestamp("note %Y-%m-%d", fixed()).unwrap(),
            "note 2026-09-04"
        );
    }

    #[test]
    fn rejects_unknown_specifiers() {
        assert!(validate_timestamp_format("%Q").is_err());
        assert!(render_timestamp("%Y-%K", fixed()).is_err());
        // A dangling percent has nothing to expand.
        assert!(validate_timestamp_format("%").is_err());
        assert!(validate_timestamp_format("logged at %").is_err());
    }

    #[test]
    fn accepts_chronos_less_obvious_specifiers() {
        // %q (quarter) and %% (a literal percent) are valid; the validator must
        // not second-guess chrono's list.
        assert_eq!(render_timestamp("Q%q %Y", fixed()).unwrap(), "Q3 2026");
        assert_eq!(render_timestamp("%H%% of %Y", fixed()).unwrap(), "17% of 2026");
        assert_eq!(render_timestamp("%Y-%m-%d %Z", fixed()).unwrap(), "2026-09-04 UTC");
    }

    #[test]
    fn rejects_an_empty_format() {
        assert!(validate_timestamp_format("").is_err());
        assert!(validate_timestamp_format("   ").is_err());
    }

    #[test]
    fn accepts_a_format_with_no_specifiers_at_all() {
        // Odd, but unambiguous: the user gets that literal text every time.
        assert_eq!(render_timestamp("logged", fixed()).unwrap(), "logged");
    }

    #[test]
    fn a_bad_format_falls_back_to_the_default_rather_than_failing() {
        let fallback = format_now("%Q");
        // Shaped like the default format: 2026-09-04T17:05:09Z
        assert_eq!(fallback.len(), 20, "unexpected fallback {fallback:?}");
        assert!(fallback.ends_with('Z'), "unexpected fallback {fallback:?}");
    }
}

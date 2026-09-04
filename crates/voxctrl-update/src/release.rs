//! Reading the latest published release from GitHub.
//!
//! One unauthenticated `GET` to the public releases API, sending nothing but a
//! `User-Agent` (which GitHub requires) and carrying no identifier of any kind.
//! It is the only request VoxCtrl makes on its own behalf, and it is the reason
//! `updates.auto_check` exists in the config — see `docs/privacy.md`.

use std::time::Duration;

use serde::{Deserialize, Serialize};

/// The repository releases are published from.
pub const RELEASES_API_URL: &str =
    "https://api.github.com/repos/JRufer/VoxCtrl/releases/latest";

/// Where a user is sent when VoxCtrl cannot update itself.
pub const RELEASES_PAGE_URL: &str = "https://github.com/JRufer/VoxCtrl/releases/latest";

/// The check has to finish, or fail, quickly: it runs at launch and nothing
/// else waits for it, but a socket left hanging for minutes on a captive-portal
/// network is a thread and a connection held for no reason.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Debug, thiserror::Error)]
pub enum UpdateError {
    #[error("could not reach GitHub: {0}")]
    Network(String),
    #[error("GitHub returned an unexpected response: {0}")]
    Response(String),
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, UpdateError>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReleaseAsset {
    pub name: String,
    pub browser_download_url: String,
    #[serde(default)]
    pub size: u64,
    /// `sha256:<hex>`, as GitHub reports it. Present on recent uploads; when it
    /// is missing the download simply is not hash-checked, which is no worse
    /// than any other HTTPS download.
    #[serde(default)]
    pub digest: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Release {
    pub tag_name: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub html_url: String,
    #[serde(default)]
    pub draft: bool,
    #[serde(default)]
    pub prerelease: bool,
    #[serde(default)]
    pub assets: Vec<ReleaseAsset>,
}

/// The HTTP client used for both the check and the download.
pub fn client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        // GitHub rejects requests without one. It names the app and its
        // version and nothing else — no machine, user or install identifier.
        .user_agent(concat!("VoxCtrl/", env!("CARGO_PKG_VERSION")))
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|e| UpdateError::Other(e.to_string()))
}

/// Fetch the newest published release.
///
/// `/releases/latest` excludes drafts and pre-releases, which is what we want:
/// the release workflow publishes drafts, and a draft is not something to offer
/// anyone.
pub async fn fetch_latest(client: &reqwest::Client) -> Result<Release> {
    fetch_latest_from(client, RELEASES_API_URL).await
}

/// [`fetch_latest`] against an explicit URL, so tests can point it at a local
/// server.
pub async fn fetch_latest_from(client: &reqwest::Client, url: &str) -> Result<Release> {
    let response = client
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .send()
        .await
        .map_err(|e| UpdateError::Network(e.to_string()))?;

    let status = response.status();
    if !status.is_success() {
        // Rate limiting is the one failure worth naming: it is transient, it
        // affects whole networks at a time, and "403" on its own tells nobody
        // that waiting is the fix.
        if status == reqwest::StatusCode::FORBIDDEN || status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err(UpdateError::Response(
                "GitHub is rate-limiting update checks from this network. Try again later."
                    .to_string(),
            ));
        }
        return Err(UpdateError::Response(format!("HTTP {status}")));
    }

    let body = response
        .text()
        .await
        .map_err(|e| UpdateError::Network(e.to_string()))?;

    parse_release(&body)
}

/// Parse a release payload, kept separate from the transport so the shape of
/// what GitHub sends can be tested without a network.
pub fn parse_release(body: &str) -> Result<Release> {
    serde_json::from_str::<Release>(body).map_err(|e| UpdateError::Response(e.to_string()))
}

/// Trim release notes to something a dialog can show without becoming a wall of
/// text, cutting at a line boundary so a heading or list item is never sliced
/// in half.
pub fn summarize_notes(body: &str, max_chars: usize) -> String {
    let body = body.replace("\r\n", "\n");
    let trimmed = body.trim();
    if trimmed.chars().count() <= max_chars {
        return trimmed.to_string();
    }

    let mut out = String::new();
    for line in trimmed.lines() {
        if out.chars().count() + line.chars().count() + 1 > max_chars {
            break;
        }
        out.push_str(line);
        out.push('\n');
    }
    if out.trim().is_empty() {
        // A single paragraph longer than the budget: take what fits.
        out = trimmed.chars().take(max_chars).collect();
    }
    format!("{}\n…", out.trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A cut-down copy of what the API actually returned for v0.3.10, including
    /// the fields we do not read — the parse must survive them.
    const SAMPLE: &str = r#"{
        "tag_name": "v0.3.10",
        "name": "VoxCtrl v0.3.10",
        "body": "What is new\n\nFixes things.",
        "html_url": "https://github.com/JRufer/VoxCtrl/releases/tag/v0.3.10",
        "draft": false,
        "prerelease": false,
        "author": {"login": "github-actions[bot]"},
        "assets": [
            {
                "name": "VoxCtrl_0.3.10_amd64-linux-x86_64.AppImage",
                "browser_download_url": "https://example.invalid/a.AppImage",
                "size": 98693616,
                "download_count": 1,
                "digest": "sha256:b0137a3a"
            }
        ]
    }"#;

    #[test]
    fn a_real_release_payload_parses() {
        let r = parse_release(SAMPLE).unwrap();
        assert_eq!(r.tag_name, "v0.3.10");
        assert!(!r.draft && !r.prerelease);
        assert_eq!(r.assets.len(), 1);
        assert_eq!(r.assets[0].size, 98693616);
        assert_eq!(r.assets[0].digest.as_deref(), Some("sha256:b0137a3a"));
    }

    #[test]
    fn a_release_without_optional_fields_still_parses() {
        let r = parse_release(r#"{"tag_name":"v1.0.0"}"#).unwrap();
        assert_eq!(r.tag_name, "v1.0.0");
        assert!(r.body.is_none());
        assert!(r.assets.is_empty());
    }

    #[test]
    fn a_non_release_response_is_an_error_not_a_panic() {
        assert!(parse_release("not json").is_err());
        assert!(parse_release(r#"{"message":"Not Found"}"#).is_err());
    }

    #[test]
    fn short_notes_are_left_alone() {
        assert_eq!(summarize_notes("  Fixes a thing.  ", 100), "Fixes a thing.");
    }

    #[test]
    fn long_notes_are_cut_at_a_line_boundary() {
        let body = "## Heading\nline one\nline two\nline three\n";
        let out = summarize_notes(body, 25);
        assert!(out.ends_with('…'));
        assert!(out.starts_with("## Heading"));
        // Never mid-line: every line kept is a whole one.
        for line in out.lines().filter(|l| *l != "…") {
            assert!(body.contains(line), "{line:?} was sliced out of the middle");
        }
    }

    #[test]
    fn one_very_long_paragraph_is_still_cut_down() {
        let body = "x".repeat(500);
        let out = summarize_notes(&body, 50);
        assert!(out.chars().count() <= 52, "got {} chars", out.chars().count());
    }

    #[test]
    fn windows_line_endings_do_not_survive_into_the_dialog() {
        let out = summarize_notes("a\r\nb\r\n", 100);
        assert_eq!(out, "a\nb");
    }
}

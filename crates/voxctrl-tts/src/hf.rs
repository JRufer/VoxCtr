//! The HuggingFace access token gated model downloads need.
//!
//! It can come from two places, and the environment wins: a token exported as
//! `HF_TOKEN` belongs to the session VoxCtrl was launched in, so a value saved
//! in the config must never quietly replace it. The config token is the
//! fallback for everyone who has not exported one — the Settings and setup
//! wizard fields write there.
//!
//! Nothing here ever writes the environment token back into the config; the UI
//! shows it, but the config keeps only what the user typed.

/// The environment variable `hf-hub` reads, and the one VoxCtrl honors first.
pub const HF_TOKEN_ENV: &str = "HF_TOKEN";

/// The token exported into the environment, if there is a usable one.
///
/// An empty or whitespace-only value counts as absent — an `HF_TOKEN=` left in
/// a shell profile should not lock the user out of the field in Settings.
pub fn hf_token_from_env() -> Option<String> {
    std::env::var(HF_TOKEN_ENV)
        .ok()
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
}

/// The token the app will actually use: the environment's, else the config's.
pub fn effective_hf_token(configured: Option<&str>) -> Option<String> {
    hf_token_from_env().or_else(|| {
        configured
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(str::to_string)
    })
}

/// Put the configured token where `hf-hub` will find it, without disturbing an
/// `HF_TOKEN` the user already exported.
///
/// Call this before any download or model load that reaches a gated repo.
pub fn apply_hf_token(configured: Option<&str>) {
    if hf_token_from_env().is_some() {
        // The environment already has one, and it wins. Leave it alone.
        return;
    }
    let Some(token) = configured.map(str::trim).filter(|t| !t.is_empty()) else {
        return;
    };
    // SAFETY: called from the download/load paths before the TTS worker thread
    // is reading the environment, matching the previous callers' contract.
    unsafe { std::env::set_var(HF_TOKEN_ENV, token) };
}

/// Marker carried by every "HuggingFace refused these credentials" error.
///
/// The UI has to tell an unusable token apart from a network problem, and the
/// difference decides what it asks the user to do — fix the token, or try
/// again later. Matching on a tag rather than on prose keeps that test from
/// breaking the next time an underlying library rewords its errors, and keeps
/// it working when the message is not in English.
pub const HF_TOKEN_REJECTED_TAG: &str = "hf-token-rejected";

/// Whether some error text is HuggingFace turning us away rather than failing.
///
/// Gated repos answer an unauthenticated or unauthorised request with 401 or
/// 403 — the same answer for "no token", "expired token" and "token belongs to
/// an account that has not accepted the licence", none of which we can tell
/// apart from out here, and all of which the user fixes in the same place.
pub fn looks_like_auth_failure(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains(HF_TOKEN_REJECTED_TAG)
        || lower.contains("401")
        || lower.contains("403")
        || lower.contains("unauthorized")
        || lower.contains("unauthorised")
        || lower.contains("authentication")
        || lower.contains("access to model")
        || lower.contains("gated repo")
}

/// The error to raise when a gated download comes back 401/403.
pub fn token_rejected(repo: &str) -> anyhow::Error {
    anyhow::anyhow!(
        "{HF_TOKEN_REJECTED_TAG}: HuggingFace did not accept the access token for {repo}. \
         Check the token is valid and that its account has accepted the model's licence."
    )
}

/// Re-label a download failure as a rejected token when that is what it is.
///
/// The gated downloads go through libraries that report an HTTP 401 as an
/// ordinary transport error, so the distinction has to be recovered from the
/// error chain rather than read off a type.
pub fn classify_download_error(err: anyhow::Error, repo: &str) -> anyhow::Error {
    if looks_like_auth_failure(&format!("{err:#}")) {
        return token_rejected(repo).context(format!("{err:#}"));
    }
    err
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `std::env` is process-wide; these tests set the same variable, so they
    /// take turns.
    fn env_lock() -> &'static std::sync::Mutex<()> {
        static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
        LOCK.get_or_init(|| std::sync::Mutex::new(()))
    }

    struct EnvGuard;

    impl EnvGuard {
        fn set(value: Option<&str>) -> Self {
            unsafe {
                match value {
                    Some(v) => std::env::set_var(HF_TOKEN_ENV, v),
                    None => std::env::remove_var(HF_TOKEN_ENV),
                }
            }
            Self
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            unsafe { std::env::remove_var(HF_TOKEN_ENV) };
        }
    }

    #[test]
    fn the_environment_wins_over_the_config() {
        let _lock = env_lock().lock().unwrap();
        let _env = EnvGuard::set(Some("hf_from_env"));

        assert_eq!(effective_hf_token(Some("hf_from_config")).as_deref(), Some("hf_from_env"));
    }

    #[test]
    fn the_config_is_used_when_the_environment_has_none() {
        let _lock = env_lock().lock().unwrap();
        let _env = EnvGuard::set(None);

        assert_eq!(effective_hf_token(Some("hf_from_config")).as_deref(), Some("hf_from_config"));
        assert_eq!(effective_hf_token(Some("   ")), None);
        assert_eq!(effective_hf_token(None), None);
    }

    #[test]
    fn an_empty_environment_value_counts_as_absent() {
        let _lock = env_lock().lock().unwrap();
        let _env = EnvGuard::set(Some("   "));

        assert_eq!(hf_token_from_env(), None);
        assert_eq!(effective_hf_token(Some("hf_from_config")).as_deref(), Some("hf_from_config"));
    }

    /// Applying the config token must not overwrite the session's own.
    #[test]
    fn applying_a_config_token_leaves_an_exported_one_intact() {
        let _lock = env_lock().lock().unwrap();
        let _env = EnvGuard::set(Some("hf_from_env"));

        apply_hf_token(Some("hf_from_config"));

        assert_eq!(std::env::var(HF_TOKEN_ENV).unwrap(), "hf_from_env");
    }

    #[test]
    fn a_refusal_is_told_apart_from_a_network_failure() {
        assert!(looks_like_auth_failure("HTTP status client error (401 Unauthorized)"));
        assert!(looks_like_auth_failure("Access to model BreezeBlue/Breeze-TTS-2 is restricted"));
        assert!(looks_like_auth_failure(&format!("{:#}", token_rejected("some/repo"))));

        assert!(!looks_like_auth_failure("error sending request: connection refused"));
        assert!(!looks_like_auth_failure("failed to write file: No space left on device"));
    }

    /// The UI keys off the tag, and it has to survive being flattened into a
    /// string on the way through Tauri's command boundary.
    #[test]
    fn a_rejected_token_is_tagged_all_the_way_through() {
        let underlying = anyhow::anyhow!("HTTP status client error (403 Forbidden)");
        let classified = classify_download_error(underlying, "some/repo");

        let text = format!("{classified:#}");
        assert!(text.contains(HF_TOKEN_REJECTED_TAG), "{text}");
        // The original failure stays in the chain: it is what a bug report needs.
        assert!(text.contains("403"), "{text}");
    }

    #[test]
    fn an_unrelated_failure_is_left_alone() {
        let underlying = anyhow::anyhow!("error sending request: connection refused");
        let classified = classify_download_error(underlying, "some/repo");

        assert!(!format!("{classified:#}").contains(HF_TOKEN_REJECTED_TAG));
    }

    #[test]
    fn applying_a_config_token_exports_it_when_nothing_is_set() {
        let _lock = env_lock().lock().unwrap();
        let _env = EnvGuard::set(None);

        apply_hf_token(Some("  hf_from_config  "));

        assert_eq!(std::env::var(HF_TOKEN_ENV).unwrap(), "hf_from_config");
    }
}

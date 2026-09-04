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
    fn applying_a_config_token_exports_it_when_nothing_is_set() {
        let _lock = env_lock().lock().unwrap();
        let _env = EnvGuard::set(None);

        apply_hf_token(Some("  hf_from_config  "));

        assert_eq!(std::env::var(HF_TOKEN_ENV).unwrap(), "hf_from_config");
    }
}

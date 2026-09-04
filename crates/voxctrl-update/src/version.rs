//! Semantic version parsing, just enough of it to answer one question:
//! is the version GitHub is offering newer than the one that is running?
//!
//! Deliberately hand-rolled rather than pulling in `semver`. The only versions
//! this ever compares are VoxCtrl's own release tags — `0.3.10`, `v0.4.0`,
//! occasionally something like `0.4.0-rc.1` — and getting that wrong in a way a
//! dependency would prevent is hard to imagine. What is easy to imagine, and
//! has bitten every hand-rolled version check ever written, is comparing
//! `0.3.10` against `0.3.9` as strings and concluding the machine is up to
//! date. That case has a test below.

use std::cmp::Ordering;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    /// The `-rc.1` in `0.4.0-rc.1`, if there is one. A version with a
    /// pre-release tag sorts *below* the same version without one.
    pub pre: Option<String>,
}

impl Version {
    /// Parse a release tag or a version string. Accepts a leading `v`, ignores
    /// `+build` metadata (semver says it takes no part in precedence), and
    /// tolerates a missing minor or patch (`1` and `1.2` both parse).
    ///
    /// Returns `None` for anything that does not start with a number, which is
    /// how a tag the release workflow never produces gets ignored rather than
    /// mistaken for an upgrade.
    pub fn parse(raw: &str) -> Option<Self> {
        let s = raw.trim();
        let s = s.strip_prefix('v').or_else(|| s.strip_prefix('V')).unwrap_or(s);
        // Build metadata never affects precedence, so drop it before anything else.
        let s = s.split('+').next().unwrap_or(s);

        let (core, pre) = match s.split_once('-') {
            Some((core, pre)) if !pre.is_empty() => (core, Some(pre.to_string())),
            _ => (s, None),
        };

        let mut parts = core.split('.');
        let major = parts.next()?.trim().parse().ok()?;
        let minor = match parts.next() {
            Some(p) => p.trim().parse().ok()?,
            None => 0,
        };
        let patch = match parts.next() {
            Some(p) => p.trim().parse().ok()?,
            None => 0,
        };
        // A fourth numeric component would mean this is not a version we know
        // how to reason about; refuse rather than silently ignore it.
        if parts.next().is_some() {
            return None;
        }

        Some(Self { major, minor, patch, pre })
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        if let Some(pre) = &self.pre {
            write!(f, "-{pre}")?;
        }
        Ok(())
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        self.major
            .cmp(&other.major)
            .then(self.minor.cmp(&other.minor))
            .then(self.patch.cmp(&other.patch))
            .then_with(|| compare_pre(self.pre.as_deref(), other.pre.as_deref()))
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Pre-release precedence, per semver: absent beats present (1.0.0 > 1.0.0-rc.1),
/// and otherwise identifiers are compared left to right, numerically where both
/// sides are numeric.
fn compare_pre(a: Option<&str>, b: Option<&str>) -> Ordering {
    match (a, b) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (Some(a), Some(b)) => {
            let mut ai = a.split('.');
            let mut bi = b.split('.');
            loop {
                match (ai.next(), bi.next()) {
                    (None, None) => return Ordering::Equal,
                    // A shorter set of identifiers sorts lower: rc < rc.1.
                    (None, Some(_)) => return Ordering::Less,
                    (Some(_), None) => return Ordering::Greater,
                    (Some(x), Some(y)) => {
                        let ord = match (x.parse::<u64>(), y.parse::<u64>()) {
                            (Ok(x), Ok(y)) => x.cmp(&y),
                            // Numeric identifiers always sort below alphanumeric ones.
                            (Ok(_), Err(_)) => Ordering::Less,
                            (Err(_), Ok(_)) => Ordering::Greater,
                            (Err(_), Err(_)) => x.cmp(y),
                        };
                        if ord != Ordering::Equal {
                            return ord;
                        }
                    }
                }
            }
        }
    }
}

/// Whether `candidate` is a version worth offering to someone running `current`.
///
/// Unparseable input on either side answers "no": an update prompt is a
/// disruption, and one raised because a tag could not be read is pure noise.
pub fn is_newer(candidate: &str, current: &str) -> bool {
    match (Version::parse(candidate), Version::parse(current)) {
        (Some(new), Some(now)) => new > now,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tag_parses_with_or_without_its_v() {
        assert_eq!(Version::parse("v0.3.10"), Version::parse("0.3.10"));
        assert_eq!(Version::parse("0.3.10").unwrap().patch, 10);
    }

    /// The bug every string comparison ships with: "0.3.10" < "0.3.9" as text.
    #[test]
    fn double_digit_patches_are_compared_as_numbers() {
        assert!(is_newer("0.3.10", "0.3.9"));
        assert!(!is_newer("0.3.9", "0.3.10"));
        assert!(is_newer("0.10.0", "0.9.12"));
    }

    #[test]
    fn the_same_version_is_not_an_update() {
        assert!(!is_newer("0.3.10", "0.3.10"));
        assert!(!is_newer("v0.3.10", "0.3.10"));
    }

    #[test]
    fn an_older_release_is_never_offered() {
        // Someone running a build newer than the published release — a local
        // build, or a release that was pulled — must not be walked backwards.
        assert!(!is_newer("0.3.9", "0.4.0"));
    }

    #[test]
    fn a_prerelease_sorts_below_its_release() {
        assert!(is_newer("0.4.0", "0.4.0-rc.1"));
        assert!(!is_newer("0.4.0-rc.1", "0.4.0"));
        assert!(is_newer("0.4.0-rc.2", "0.4.0-rc.1"));
        assert!(is_newer("0.4.0-rc.1", "0.3.10"));
    }

    #[test]
    fn build_metadata_is_ignored() {
        assert!(!is_newer("0.3.10+abc123", "0.3.10"));
    }

    #[test]
    fn a_short_version_fills_in_zeros() {
        assert_eq!(Version::parse("1").unwrap(), Version::parse("1.0.0").unwrap());
        assert_eq!(Version::parse("1.2").unwrap(), Version::parse("1.2.0").unwrap());
    }

    #[test]
    fn garbage_is_never_an_update() {
        for tag in ["", "nightly", "latest", "v", "1.2.3.4", "x.y.z"] {
            assert!(!is_newer(tag, "0.3.10"), "{tag} must not read as an update");
            assert!(!is_newer("0.4.0", tag), "a bad current version must not prompt");
        }
    }

    #[test]
    fn display_round_trips() {
        for s in ["0.3.10", "1.0.0", "0.4.0-rc.1"] {
            assert_eq!(Version::parse(s).unwrap().to_string(), s);
        }
    }
}

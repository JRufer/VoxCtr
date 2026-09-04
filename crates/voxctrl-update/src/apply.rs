//! Downloading a release asset and putting it in place.
//!
//! The order of operations here is the whole point. A half-written AppImage
//! that has replaced a working one is an app that no longer starts, on a
//! machine whose owner did nothing wrong except say yes to an update. So the
//! new file is written beside the old one under a temporary name, hash-checked
//! against what GitHub says it published, made executable, and only then moved
//! over the top in a single `rename` — which on the same filesystem either
//! happens or does not. Nothing is deleted at any point.

use std::io::Write;
use std::path::{Path, PathBuf};

use futures_util::StreamExt;
use sha2::{Digest, Sha256};

use crate::release::{ReleaseAsset, Result, UpdateError};

/// Progress of a download, in bytes. `total` is 0 when the server sends no
/// length — the UI shows an indeterminate bar rather than a wrong percentage.
#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct Progress {
    pub downloaded: u64,
    pub total: u64,
}

impl Progress {
    pub fn percent(&self) -> Option<f64> {
        if self.total == 0 {
            None
        } else {
            Some((self.downloaded as f64 / self.total as f64) * 100.0)
        }
    }
}

/// Name of the scratch file written next to the target during an update.
///
/// Hidden and process-scoped: two VoxCtrl processes updating the same file at
/// once is not a thing that should happen, but if it does they must not write
/// to the same scratch file.
pub fn staging_name(target: &Path) -> String {
    let stem = target
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "VoxCtrl".to_string());
    format!(".{stem}.update-{}", std::process::id())
}

/// Fail before downloading anything if the new file could never be put in
/// place.
///
/// An AppImage in `/opt`, or on a read-only mount, cannot be replaced by an
/// unprivileged process. Finding that out after a 100 MB download — and after
/// the user has watched a progress bar to 100% — is a bad way to learn it.
pub fn check_writable(target: &Path) -> Result<()> {
    let dir = target.parent().ok_or_else(|| {
        UpdateError::Other(format!("{} has no parent directory", target.display()))
    })?;

    let probe = dir.join(format!(".voxctrl-write-test-{}", std::process::id()));
    match std::fs::File::create(&probe) {
        Ok(_) => {
            let _ = std::fs::remove_file(&probe);
        }
        Err(e) => {
            return Err(UpdateError::Other(format!(
                "VoxCtrl cannot write to {} ({e}). Move the application file somewhere you own \
                 — your home directory, for instance — or download the new release by hand.",
                dir.display()
            )))
        }
    }

    // A file that exists but cannot be replaced (an immutable flag, a
    // read-only file on a writable directory) still fails the rename.
    if let Ok(meta) = std::fs::metadata(target) {
        if meta.permissions().readonly() {
            return Err(UpdateError::Other(format!(
                "{} is read-only, so VoxCtrl cannot replace it.",
                target.display()
            )));
        }
    }

    Ok(())
}

/// Download `asset` to `dest`, reporting progress as it goes.
///
/// Streamed rather than buffered: the AppImage is around 100 MB, and holding
/// all of it in memory on a machine that is also running speech inference is
/// gratuitous.
pub async fn download(
    client: &reqwest::Client,
    asset: &ReleaseAsset,
    dest: &Path,
    mut on_progress: impl FnMut(Progress),
) -> Result<()> {
    let response = client
        .get(&asset.browser_download_url)
        .send()
        .await
        .map_err(|e| UpdateError::Network(e.to_string()))?;

    if !response.status().is_success() {
        return Err(UpdateError::Response(format!(
            "HTTP {} downloading {}",
            response.status(),
            asset.name
        )));
    }

    let total = response.content_length().unwrap_or(asset.size);
    let mut file = std::fs::File::create(dest)
        .map_err(|e| UpdateError::Other(format!("could not create {}: {e}", dest.display())))?;

    let mut downloaded: u64 = 0;
    let mut last_reported: u64 = 0;
    on_progress(Progress { downloaded: 0, total });

    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| UpdateError::Network(e.to_string()))?;
        file.write_all(&chunk)
            .map_err(|e| UpdateError::Other(format!("could not write {}: {e}", dest.display())))?;
        downloaded += chunk.len() as u64;

        // One event per megabyte. At a chunk apiece this would be tens of
        // thousands of IPC messages for one download, which costs more than the
        // download does.
        if downloaded - last_reported >= 1_048_576 {
            last_reported = downloaded;
            on_progress(Progress { downloaded, total });
        }
    }

    file.flush()
        .map_err(|e| UpdateError::Other(format!("could not flush {}: {e}", dest.display())))?;
    // The rename below publishes this file as the application itself. Without a
    // sync, a machine that loses power just after the rename can come back to a
    // correctly-named file with no contents.
    file.sync_all()
        .map_err(|e| UpdateError::Other(format!("could not sync {}: {e}", dest.display())))?;
    drop(file);

    on_progress(Progress { downloaded, total: if total == 0 { downloaded } else { total } });
    Ok(())
}

/// SHA-256 of a file, lowercase hex.
pub fn sha256_file(path: &Path) -> Result<String> {
    let mut file = std::fs::File::open(path)
        .map_err(|e| UpdateError::Other(format!("could not read {}: {e}", path.display())))?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher)
        .map_err(|e| UpdateError::Other(format!("could not read {}: {e}", path.display())))?;
    Ok(hex::encode(hasher.finalize()))
}

/// [`verify_digest`] off the async runtime.
///
/// Hashing 100 MB takes a noticeable fraction of a second, and this runs on the
/// same runtime that drives the UI's IPC — long enough to be felt as a stall if
/// it were done inline.
pub async fn verify_digest_blocking(path: &Path, digest: Option<&str>) -> Result<()> {
    let path = path.to_path_buf();
    let digest = digest.map(str::to_string);
    tokio::task::spawn_blocking(move || verify_digest(&path, digest.as_deref()))
        .await
        .map_err(|e| UpdateError::Other(format!("checksum task failed: {e}")))?
}

/// Check a downloaded file against the digest GitHub published for it.
///
/// A release asset without a digest is not treated as a failure — GitHub only
/// started reporting them recently, and refusing to update from an older
/// release would break the feature for exactly the people who most need it.
/// A digest that is present and does not match is fatal.
pub fn verify_digest(path: &Path, digest: Option<&str>) -> Result<()> {
    let Some(digest) = digest else {
        tracing::warn!("release asset carries no digest; skipping hash check");
        return Ok(());
    };
    let Some(expected) = digest.strip_prefix("sha256:") else {
        tracing::warn!("unrecognised digest format {digest:?}; skipping hash check");
        return Ok(());
    };

    let actual = sha256_file(path)?;
    if !actual.eq_ignore_ascii_case(expected.trim()) {
        return Err(UpdateError::Other(format!(
            "the downloaded file does not match the checksum GitHub published for it \
             (expected {expected}, got {actual}). The update has not been installed."
        )));
    }
    Ok(())
}

/// Make the staged file executable and move it over the running AppImage.
pub fn install_appimage(staged: &Path, target: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // An AppImage that is not executable is a file the desktop cannot
        // launch, so this has to happen before the rename, not after.
        std::fs::set_permissions(staged, std::fs::Permissions::from_mode(0o755)).map_err(|e| {
            UpdateError::Other(format!("could not make {} executable: {e}", staged.display()))
        })?;
    }

    // Same directory by construction, so this is an atomic replace: the file at
    // `target` is either entirely the old version or entirely the new one, and
    // the running process keeps its own open image either way.
    std::fs::rename(staged, target).map_err(|e| {
        UpdateError::Other(format!(
            "could not replace {} ({e}). The update was downloaded but not installed.",
            target.display()
        ))
    })
}

/// Where to stage the download for a given target file.
pub fn staging_path(target: &Path) -> PathBuf {
    let dir = target.parent().unwrap_or_else(|| Path::new("."));
    dir.join(staging_name(target))
}

/// Relaunch the application once this process has exited.
///
/// The wait matters. VoxCtrl runs as a single instance: a second copy started
/// while the first is alive hands its arguments to the running one and quits
/// immediately. Spawning the new build and exiting in the same breath is a race
/// that, when lost, leaves the user staring at the old version and concluding
/// the update did nothing. So the relaunch is handed to a detached shell that
/// waits for this process to actually be gone before starting anything.
#[cfg(unix)]
pub fn spawn_relaunch(path: &Path) -> Result<()> {
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};

    let pid = std::process::id().to_string();
    let script = r#"
pid="$1"; shift
# Poll rather than `wait`: this shell is not the parent of the process it is
# waiting for, so it cannot be waited on. Give up after 30s and start anyway
# rather than leaving the user with nothing running.
i=0
while kill -0 "$pid" 2>/dev/null && [ "$i" -lt 300 ]; do
  sleep 0.1
  i=$((i+1))
done
exec "$@"
"#;

    let mut cmd = Command::new("sh");
    cmd.arg("-c")
        .arg(script)
        .arg("sh") // $0
        .arg(&pid)
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    // Its own process group, so the relaunch is not killed along with this
    // process when the session tears down.
    cmd.process_group(0);

    cmd.spawn()
        .map(|_| ())
        .map_err(|e| UpdateError::Other(format!("could not relaunch VoxCtrl: {e}")))
}

/// Windows: hand off to the downloaded installer, which replaces the install
/// and can start the new build itself. Nothing waits for this process, because
/// the installer already knows how to deal with a running copy.
#[cfg(windows)]
pub fn spawn_relaunch(path: &Path) -> Result<()> {
    use std::process::{Command, Stdio};

    Command::new(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| UpdateError::Other(format!("could not start the installer: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn the_staging_file_sits_beside_the_target_and_is_hidden() {
        let target = Path::new("/home/u/Apps/VoxCtrl.AppImage");
        let staged = staging_path(target);
        assert_eq!(staged.parent(), target.parent());
        let name = staged.file_name().unwrap().to_string_lossy().to_string();
        assert!(name.starts_with('.'), "{name} should be hidden");
        assert_ne!(staged, target, "staging must never be the target itself");
    }

    #[test]
    fn a_writable_directory_passes_the_check() {
        let dir = tempdir().unwrap();
        let target = dir.path().join("VoxCtrl.AppImage");
        std::fs::write(&target, b"old").unwrap();
        assert!(check_writable(&target).is_ok());
    }

    #[test]
    fn a_missing_directory_is_reported_before_any_download() {
        let target = Path::new("/definitely/not/here/VoxCtrl.AppImage");
        let err = check_writable(target).unwrap_err().to_string();
        assert!(err.contains("cannot write"), "unhelpful error: {err}");
    }

    #[test]
    fn a_matching_digest_verifies() {
        let dir = tempdir().unwrap();
        let f = dir.path().join("payload");
        std::fs::write(&f, b"hello").unwrap();
        let sum = sha256_file(&f).unwrap();
        assert_eq!(
            sum,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
        assert!(verify_digest(&f, Some(&format!("sha256:{sum}"))).is_ok());
        assert!(verify_digest(&f, Some(&format!("sha256:{}", sum.to_uppercase()))).is_ok());
    }

    #[test]
    fn a_mismatched_digest_stops_the_update() {
        let dir = tempdir().unwrap();
        let f = dir.path().join("payload");
        std::fs::write(&f, b"tampered").unwrap();
        let err = verify_digest(&f, Some("sha256:0000000000000000")).unwrap_err().to_string();
        assert!(err.contains("checksum"), "{err}");
        assert!(err.contains("not been installed"), "{err}");
    }

    /// Older releases predate GitHub reporting digests. Refusing to update from
    /// them would strand the users furthest behind.
    #[test]
    fn a_missing_digest_is_not_a_failure() {
        let dir = tempdir().unwrap();
        let f = dir.path().join("payload");
        std::fs::write(&f, b"whatever").unwrap();
        assert!(verify_digest(&f, None).is_ok());
        assert!(verify_digest(&f, Some("md5:abc")).is_ok());
    }

    #[test]
    fn installing_replaces_the_target_atomically_and_keeps_it_executable() {
        let dir = tempdir().unwrap();
        let target = dir.path().join("VoxCtrl.AppImage");
        std::fs::write(&target, b"old version").unwrap();
        let staged = staging_path(&target);
        std::fs::write(&staged, b"new version").unwrap();

        install_appimage(&staged, &target).unwrap();

        assert_eq!(std::fs::read(&target).unwrap(), b"new version");
        assert!(!staged.exists(), "the staging file must not be left behind");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&target).unwrap().permissions().mode();
            assert_eq!(mode & 0o111, 0o111, "the new AppImage must be executable");
        }
    }

    #[test]
    fn progress_reports_a_percentage_only_when_the_size_is_known() {
        assert_eq!(Progress { downloaded: 50, total: 200 }.percent(), Some(25.0));
        assert_eq!(Progress { downloaded: 50, total: 0 }.percent(), None);
    }
}

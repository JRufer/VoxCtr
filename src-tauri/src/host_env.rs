//! Running host programs from inside a bundled app.
//!
//! VoxCtrl shells out to programs that belong to the user's desktop, not to
//! VoxCtrl: `gsettings` to read and write Cinnamon's keybindings, `dbus-send`
//! to poke the running instance. Inside the AppImage those inherit an
//! environment aimed squarely at the *bundled* runtime — linuxdeploy's GTK hook
//! exports `GSETTINGS_SCHEMA_DIR` into the AppDir, prepends the AppDir to
//! `XDG_DATA_DIRS`, and puts bundled libraries first on `LD_LIBRARY_PATH`.
//!
//! A `gsettings` that inherits all that looks in the AppDir for the Cinnamon
//! schemas, does not find them, and reports that the desktop has no keybinding
//! support at all — which is indistinguishable, from the outside, from running
//! on a desktop that really has none. Handing host programs a host environment
//! is what makes the difference.

use std::process::Command;

/// Variables that point a GLib/GTK program at a bundled runtime. Cleared
/// outright for a host program: with them unset GLib falls back to the
/// XDG defaults, which is where the desktop's own files live.
const BUNDLE_ONLY_VARS: &[&str] = &[
    "GSETTINGS_SCHEMA_DIR",
    "GIO_MODULE_DIR",
    "GTK_PATH",
    "GTK_DATA_PREFIX",
    "GTK_EXE_PREFIX",
    "GTK_IM_MODULE_FILE",
    "GDK_PIXBUF_MODULE_FILE",
    "GDK_PIXBUF_MODULEDIR",
    "GI_TYPELIB_PATH",
    "LD_PRELOAD",
    "PYTHONHOME",
    "PYTHONPATH",
    "WEBKIT_EXEC_PATH",
    "WEBKIT_INJECTED_BUNDLE_PATH",
];

/// Variables that are a search path the host also has a legitimate stake in.
/// Only the AppDir entries are removed; whatever the session set is kept.
const PATH_LIST_VARS: &[&str] = &["XDG_DATA_DIRS", "LD_LIBRARY_PATH", "XDG_CONFIG_DIRS"];

/// A `Command` for a program that belongs to the host, not to this bundle.
pub fn host_command(program: &str) -> Command {
    let mut cmd = Command::new(program);
    apply_host_env(&mut cmd, std::env::var("APPDIR").ok().as_deref());
    cmd
}

/// Strip the bundle out of a command's inherited environment.
///
/// Split from `host_command` so the rules can be tested without spawning
/// anything or mutating this process's own environment.
pub fn apply_host_env(cmd: &mut Command, appdir: Option<&str>) {
    for var in BUNDLE_ONLY_VARS {
        cmd.env_remove(var);
    }

    // Outside an AppImage there is no AppDir to strip, and the search paths are
    // already the host's own.
    let Some(appdir) = appdir.filter(|d| !d.is_empty()) else {
        return;
    };

    for var in PATH_LIST_VARS {
        let Ok(value) = std::env::var(var) else {
            continue;
        };
        match strip_prefix_entries(&value, appdir) {
            // Every entry came from the bundle. Unsetting beats setting an
            // empty string, which GLib reads as "no directories" rather than
            // "use the defaults".
            Some(kept) if kept.is_empty() => {
                cmd.env_remove(var);
            }
            Some(kept) => {
                cmd.env(var, kept);
            }
            None => {}
        }
    }
}

/// Drop the `:`-separated entries that live under `prefix`. `None` when nothing
/// was dropped, so the caller can leave the variable exactly as it found it.
fn strip_prefix_entries(value: &str, prefix: &str) -> Option<String> {
    let kept: Vec<&str> = value
        .split(':')
        .filter(|entry| !entry.is_empty() && !entry.starts_with(prefix))
        .collect();
    if kept.len() == value.split(':').filter(|e| !e.is_empty()).count() {
        return None;
    }
    Some(kept.join(":"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundle_directories_are_dropped_from_a_search_path() {
        let stripped = strip_prefix_entries(
            "/tmp/.mount_Vox123/usr/share:/usr/local/share:/usr/share",
            "/tmp/.mount_Vox123",
        );
        assert_eq!(stripped.as_deref(), Some("/usr/local/share:/usr/share"));
    }

    #[test]
    fn a_path_with_nothing_from_the_bundle_is_left_alone() {
        // Returning Some here would rewrite the variable for no reason, and
        // an unnecessary rewrite is a chance to get it wrong.
        assert_eq!(
            strip_prefix_entries("/usr/local/share:/usr/share", "/tmp/.mount_Vox123"),
            None
        );
    }

    #[test]
    fn a_path_that_is_entirely_bundle_collapses_to_nothing() {
        // The caller unsets the variable in this case: an empty string would
        // tell GLib there are no data directories at all, rather than to use
        // its defaults.
        assert_eq!(
            strip_prefix_entries("/tmp/.mount_Vox123/usr/share", "/tmp/.mount_Vox123").as_deref(),
            Some("")
        );
    }

    #[test]
    fn empty_entries_never_survive_the_strip() {
        // A trailing colon leaves an empty entry, which GLib reads as the
        // current directory.
        assert_eq!(
            strip_prefix_entries("/tmp/.mount_Vox123/usr/share::/usr/share", "/tmp/.mount_Vox123")
                .as_deref(),
            Some("/usr/share")
        );
    }
}

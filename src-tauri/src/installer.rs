use std::path::PathBuf;
use std::process::Command;

pub fn detect_pkg_manager() -> &'static str {
    #[cfg(test)]
    {
        if let Ok(mock) = std::env::var("VOXCTRL_PKG_MANAGER_MOCK") {
            return match mock.as_str() {
                "pacman" => "pacman",
                "apt" => "apt",
                "dnf" => "dnf",
                "zypper" => "zypper",
                "unknown" => "unknown",
                _ => "apt",
            };
        }
    }

    // Cached: the setup status is polled while the setup window is open, and
    // probing four package managers per poll is pure waste — the answer cannot
    // change while the app is running.
    static DETECTED: std::sync::OnceLock<&'static str> = std::sync::OnceLock::new();
    DETECTED.get_or_init(|| {
        if Command::new("pacman").arg("--version").output().is_ok() {
            "pacman"
        } else if Command::new("apt-get").arg("--version").output().is_ok() {
            "apt"
        } else if Command::new("dnf").arg("--version").output().is_ok() {
            "dnf"
        } else if Command::new("zypper").arg("--version").output().is_ok() {
            "zypper"
        } else {
            "unknown"
        }
    })
}

pub fn get_install_packages_command(pkg_mgr: &str) -> Option<String> {
    match pkg_mgr {
        "pacman" => Some("pacman -S --noconfirm --needed webkit2gtk-4.1 openssl libayatana-appindicator wtype xdotool wl-clipboard xclip portaudio espeak-ng".to_string()),
        "apt" => Some("apt-get update -y && apt-get install -y libwebkit2gtk-4.1-0 libssl3 libayatana-appindicator3-1 wtype xdotool wl-clipboard xclip libportaudio2 espeak-ng".to_string()),
        "dnf" => Some("dnf install -y webkit2gtk4.1 openssl libayatana-appindicator3 wtype xdotool wl-clipboard xclip portaudio espeak-ng".to_string()),
        "zypper" => Some("zypper install -y libwebkit2gtk-4_1-0 libopenssl3 libayatana-appindicator3-1 wtype xdotool wl-clipboard xclip libportaudio2 espeak-ng".to_string()),
        _ => None,
    }
}

/// Path of the udev rule VoxCtrl installs.
pub const UDEV_RULE_PATH: &str = "/etc/udev/rules.d/99-voxctrl.rules";

/// The udev rule that grants VoxCtrl access to the keyboard.
///
/// The `uaccess` tag is what makes the install restart-free. systemd-logind
/// applies an ACL for whoever owns the active local session as soon as the rule
/// is reloaded and the devices are re-triggered, so hotkeys start working
/// immediately — no logout, no reboot. `usermod -aG input` is kept as a
/// fallback for systems without logind, but on a normal desktop it is no longer
/// the mechanism the user depends on, which is precisely why they no longer
/// have to log out for it to take effect.
pub const UDEV_RULE_CONTENT: &str = r#"# Installed by VoxCtrl — global hotkey and input access.
#
# uaccess grants the user of the ACTIVE local session an ACL on these devices,
# applied immediately by systemd-logind on `udevadm trigger`. This is why
# VoxCtrl does not require logging out after setup, and it is narrower than
# permanent `input` group membership: access follows the seat, not the account.
SUBSYSTEM=="input", KERNEL=="event*", TAG+="uaccess"

# Virtual keyboard device used for synthetic keystroke injection.
KERNEL=="uinput", SUBSYSTEM=="misc", GROUP="input", MODE="0660", OPTIONS+="static_node=uinput", TAG+="uaccess"
"#;

/// True if an already-installed rule file grants immediate (logout-free)
/// access. Older VoxCtrl versions wrote a group-only rule, so an upgrading
/// user needs the setup re-run to get the no-logout behaviour.
pub fn rule_content_is_current(content: &str) -> bool {
    content.contains("uaccess") && content.contains("SUBSYSTEM==\"input\"")
}

/// Builds the privileged setup script shared by the CLI and GUI installers.
///
/// Ordering matters. The hotkey-permission steps (udev rule, rule reload,
/// input-group membership) come FIRST and determine the script's exit status.
/// The host package installation runs last as best-effort: on rolling distros
/// (Arch/CachyOS) a stale mirror or out-of-date sync database makes the package
/// step fail routinely, and that must not abort the permission setup —
/// otherwise the startup permissions warning reappears forever even though the
/// user ran the setup as instructed.
///
/// Writing the rule file is the only step allowed to fail the script. Whether
/// hotkeys actually work is not something an exit code can answer, so the app
/// verifies it afterwards by trying to open a real input device.
pub fn build_privileged_setup_script(pkg_mgr: &str, username: &str) -> String {
    let mut script = format!(
        r#"set -u
mkdir -p /etc/udev/rules.d || exit 1
cat > {rule_path} <<'VOXCTRL_RULE_EOF' || exit 1
{rule_content}VOXCTRL_RULE_EOF
chmod 0644 {rule_path} || exit 1
udevadm control --reload-rules || true
udevadm trigger --subsystem-match=input --action=change || true
udevadm trigger --subsystem-match=misc --action=change || true
"#,
        rule_path = UDEV_RULE_PATH,
        rule_content = UDEV_RULE_CONTENT,
    );

    if !username.is_empty() {
        // Fallback for non-logind systems, and a safety net if uaccess does not
        // apply (e.g. a remote session that logind does not consider active).
        script.push_str(&format!("usermod -aG input {} || true\n", username));
    }

    if let Some(cmd) = get_install_packages_command(pkg_mgr) {
        script.push_str(&format!(
            "{{ {cmd}; }} || echo 'VoxCtrl: host package installation failed (mirrors/network?). Hotkey permissions were still configured; install the packages manually later.' >&2\n"
        ));
    }

    script.push_str("exit 0\n");
    script
}

/// The commands a user can run by hand if the graphical setup cannot run
/// (no pkexec, no polkit agent, locked-down machine). Shown verbatim in the
/// setup window so a failure is always recoverable without hunting the docs.
pub fn manual_setup_commands(pkg_mgr: &str, username: &str) -> String {
    let mut lines = vec![
        format!("sudo tee {UDEV_RULE_PATH} > /dev/null <<'EOF'"),
        UDEV_RULE_CONTENT.trim_end().to_string(),
        "EOF".to_string(),
        "sudo udevadm control --reload-rules".to_string(),
        "sudo udevadm trigger --subsystem-match=input --action=change".to_string(),
    ];
    if !username.is_empty() {
        lines.push(format!("sudo usermod -aG input {username}"));
    }
    if let Some(cmd) = get_install_packages_command(pkg_mgr) {
        lines.push(format!("sudo sh -c '{cmd}'"));
    }
    lines.join("\n")
}

/// Environment marker set on the child of a group relaunch, so a machine where
/// the relaunch does not actually help cannot spawn VoxCtrl forever.
pub const RELAUNCH_MARKER_ENV: &str = "VOXCTRL_INPUT_GROUP_RELAUNCH";

pub fn command_exists(cmd: &str) -> bool {
    #[cfg(test)]
    {
        if let Ok(list) = std::env::var("VOXCTRL_FAKE_COMMANDS") {
            return list.split(',').any(|c| c == cmd);
        }
    }
    voxctrl_config::find_in_path(cmd).is_some()
}

/// Is the user recorded in the system `input` group, regardless of whether
/// their current login session picked it up?
pub fn user_in_input_group_db() -> bool {
    let Ok(username) = std::env::var("USER") else {
        return false;
    };
    if username.is_empty() {
        return false;
    }
    match Command::new("id").args(["-Gn", &username]).output() {
        Ok(out) => String::from_utf8_lossy(&out.stdout)
            .split_whitespace()
            .any(|g| g == "input"),
        Err(_) => false,
    }
}

/// Actually run `sg` and check that the resulting process really does hold the
/// `input` group.
///
/// A probe rather than an assumption: if the group is password-protected, or
/// PAM refuses, `sg` fails or blocks on a prompt that a GUI launch can never
/// answer. Discovering that *after* telling the user "restart to finish" — and
/// exiting the only running instance — would leave them with an app that
/// simply does not come back.
fn sg_grants_input_group() -> bool {
    static PROBE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *PROBE.get_or_init(|| {
        #[cfg(test)]
        {
            if std::env::var_os("VOXCTRL_FAKE_COMMANDS").is_some() {
                return true;
            }
        }
        Command::new("sg")
            .args(["input", "-c", "id -Gn"])
            .stdin(std::process::Stdio::null())
            .output()
            .map(|out| {
                out.status.success()
                    && String::from_utf8_lossy(&out.stdout)
                        .split_whitespace()
                        .any(|g| g == "input")
            })
            .unwrap_or(false)
    })
}

/// Can VoxCtrl pick up its new `input` group membership by relaunching itself,
/// instead of asking the user to log out of their desktop session?
///
/// `sg` runs a command with an additional group taken from `/etc/group`, which
/// is exactly the membership `usermod -aG` just wrote. That sidesteps the whole
/// "log out and back in" step: the only reason a fresh login was ever needed is
/// that a session's credentials are fixed at login time.
pub fn can_relaunch_with_input_group() -> bool {
    std::env::var_os(RELAUNCH_MARKER_ENV).is_none()
        && command_exists("sg")
        && user_in_input_group_db()
        && sg_grants_input_group()
}

fn app_executable_path() -> String {
    // Inside an AppImage, current_exe() points at the extracted mount, which
    // vanishes when the process exits. $APPIMAGE is the launcher to re-run.
    std::env::var("APPIMAGE").unwrap_or_else(|_| {
        std::env::current_exe()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| "voxctrl".to_string())
    })
}

fn shell_single_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// Builds the shell command handed to `sg input -c`.
///
/// It waits for the current process to exit before starting the replacement:
/// VoxCtrl runs behind a single-instance guard, so a new copy launched while
/// the old one is alive would just focus the old window and exit.
pub fn build_relaunch_command(exe: &str, pid: u32) -> String {
    format!(
        "while kill -0 {pid} 2>/dev/null; do sleep 0.2; done; exec {}",
        shell_single_quote(exe)
    )
}

/// Startup recovery for the case that used to require logging out.
///
/// If a previous setup added the user to the `input` group but this login
/// session never received it, VoxCtrl restarts itself through `sg` before any
/// window appears. The user sees a working app instead of a dialog telling them
/// to log out of their desktop. Returns true when a replacement was launched
/// and the caller should exit.
pub fn auto_relaunch_if_pending() -> bool {
    if crate::commands::any_input_device_readable() {
        return false;
    }
    if !can_relaunch_with_input_group() {
        return false;
    }

    tracing::info!(
        "Input group membership is not active in this session; relaunching VoxCtrl \
         through `sg input` so hotkeys work without logging out"
    );
    match relaunch_with_input_group() {
        Ok(()) => true,
        Err(e) => {
            tracing::warn!("Automatic permission relaunch failed: {e}");
            false
        }
    }
}

/// Relaunch VoxCtrl with the `input` group applied. The caller is expected to
/// shut the current instance down immediately afterwards.
pub fn relaunch_with_input_group() -> Result<(), String> {
    let exe = app_executable_path();
    let cmd = build_relaunch_command(&exe, std::process::id());

    #[cfg(test)]
    {
        if std::env::var_os("VOXCTRL_INSTALLER_TEST_MOCK").is_some() {
            return Ok(());
        }
    }

    Command::new("sg")
        .arg("input")
        .arg("-c")
        .arg(&cmd)
        .env(RELAUNCH_MARKER_ENV, "1")
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Could not relaunch VoxCtrl with input group access: {e}"))
}

fn run_command_status(runner: &str, args: &[&str]) -> Result<std::process::ExitStatus, String> {
    #[cfg(test)]
    {
        if let Ok(mock) = std::env::var("VOXCTRL_INSTALLER_TEST_MOCK") {
            if mock == "success" {
                #[cfg(unix)]
                {
                    use std::os::unix::process::ExitStatusExt;
                    return Ok(std::process::ExitStatus::from_raw(0));
                }
                #[cfg(not(unix))]
                {
                    return Ok(Command::new("cmd").args(&["/c", "exit 0"]).status().map_err(|e| e.to_string())?);
                }
            } else if mock == "failure" {
                #[cfg(unix)]
                {
                    use std::os::unix::process::ExitStatusExt;
                    return Ok(std::process::ExitStatus::from_raw(256)); // exit status 1
                }
                #[cfg(not(unix))]
                {
                    return Ok(Command::new("cmd").args(&["/c", "exit 1"]).status().map_err(|e| e.to_string())?);
                }
            } else if mock == "spawn_error" {
                return Err("Failed to spawn command".to_string());
            }
        }
    }

    Command::new(runner)
        .args(args)
        .status()
        .map_err(|e| format!("Failed to spawn {}: {}", runner, e))
}

pub fn setup_desktop_integration() -> Result<(), String> {
    let home_dir = dirs::home_dir().ok_or("Could not find home directory")?;
    let launcher_dir = home_dir.join(".local/share/applications");
    let icon_dir = home_dir.join(".local/share/icons/hicolor/128x128/apps");

    std::fs::create_dir_all(&launcher_dir).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&icon_dir).map_err(|e| e.to_string())?;

    // Copy high-res icon
    let icon_path = icon_dir.join("voxctrl.png");
    let icon_bytes = include_bytes!("../icons/128x128.png");
    std::fs::write(&icon_path, icon_bytes).map_err(|e| e.to_string())?;

    // Create desktop launcher
    let appimage_path = std::env::var("APPIMAGE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::current_exe().unwrap_or_default());
    
    let abs_path = std::fs::canonicalize(&appimage_path)
        .unwrap_or(appimage_path);

    let desktop_content = format!(
        r#"[Desktop Entry]
Name=VoxCtrl
Comment=Private Global Voice Dictation Gateway
Exec={}
Icon=voxctrl
Terminal=false
Type=Application
Categories=Utility;AudioVideo;
StartupNotify=false
StartupWMClass=ai.voxctrl.app
Keywords=whisper;voice;dictation;wayland;
"#,
        abs_path.to_string_lossy()
    );

    let launcher_path = launcher_dir.join("voxctrl.desktop");
    std::fs::write(&launcher_path, desktop_content).map_err(|e| e.to_string())?;
    
    // Make desktop entry executable
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(metadata) = std::fs::metadata(&launcher_path) {
            let mut perms = metadata.permissions();
            perms.set_mode(0o755);
            let _ = std::fs::set_permissions(&launcher_path, perms);
        }
    }

    Ok(())
}

pub fn run_cli_installer() -> Result<(), String> {
    println!("=== VoxCtrl CLI Installer & Host Setup ===");
    let pkg_mgr = detect_pkg_manager();
    println!("Detected Package Manager: {}", pkg_mgr);

    let username = std::env::var("USER").unwrap_or_default();
    let full_script = build_privileged_setup_script(pkg_mgr, &username);
    println!("Preparing to run system setup command via sudo...");
    println!("Executing: sudo sh -c \"{}\"", full_script);

    let status = run_command_status("sudo", &["sh", "-c", &full_script])?;

    if !status.success() {
        return Err("udev rules / input group permission setup failed.".to_string());
    }

    println!("System dependencies and udev rules configured successfully!");

    // Setup desktop integration
    println!("Registering desktop entry and icon...");
    setup_desktop_integration()?;
    println!("Desktop integration complete!");

    println!("\n==================================================");
    println!("  Setup & Integration Complete!");
    println!("==================================================");

    // Report what actually happened rather than assuming the worst. The udev
    // `uaccess` rule normally takes effect the moment the rules are reloaded,
    // so most users are done here.
    match verify_hotkey_access() {
        HotkeyAccess::Working => {
            println!("Keyboard access is live — global hotkeys work right now.");
        }
        HotkeyAccess::NeedsRelaunch => {
            println!("Keyboard access will be picked up the next time VoxCtrl starts.");
            println!("Just start VoxCtrl — you do NOT need to log out.");
        }
        HotkeyAccess::NeedsRelogin => {
            println!("Your session could not pick up the new permissions automatically.");
            println!("Please log out and log back in (or reboot) for evdev hotkeys to work.");
        }
    }
    Ok(())
}

/// What still stands between the user and working global hotkeys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyAccess {
    /// Input devices are readable in this process right now.
    Working,
    /// Not readable here, but a relaunch through `sg input` will fix it.
    NeedsRelaunch,
    /// Nothing the app can do by itself; the session must be restarted.
    NeedsRelogin,
}

pub fn verify_hotkey_access() -> HotkeyAccess {
    if crate::commands::any_input_device_readable() {
        HotkeyAccess::Working
    } else if can_relaunch_with_input_group() {
        HotkeyAccess::NeedsRelaunch
    } else {
        HotkeyAccess::NeedsRelogin
    }
}

pub async fn run_gui_installer() -> Result<(), String> {
    let pkg_mgr = detect_pkg_manager();
    let username = std::env::var("USER").unwrap_or_default();
    let full_script = build_privileged_setup_script(pkg_mgr, &username);

    if !command_exists("pkexec") {
        return Err(
            "pkexec is not installed, so VoxCtrl cannot ask for administrator access. \
             Run the commands shown under \"Set it up manually\" in a terminal instead."
                .to_string(),
        );
    }

    let script_clone = full_script.clone();
    let status = tokio::task::spawn_blocking(move || {
        run_command_status("pkexec", &["sh", "-c", &script_clone])
    }).await.map_err(|e| format!("Spawn error: {}", e))??;

    if !status.success() {
        return Err("Permission setup (udev rules / input group) failed or was canceled.".to_string());
    }

    // Desktop integration
    setup_desktop_integration()?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use std::fs;

    #[test]
    fn test_get_install_packages_command() {
        let pacman_cmd = get_install_packages_command("pacman");
        assert!(pacman_cmd.is_some());
        assert!(pacman_cmd.unwrap().contains("pacman -S"));

        let apt_cmd = get_install_packages_command("apt");
        assert!(apt_cmd.is_some());
        assert!(apt_cmd.unwrap().contains("apt-get install"));

        let unknown_cmd = get_install_packages_command("unknown");
        assert!(unknown_cmd.is_none());
    }

    #[test]
    fn test_privileged_script_orders_permissions_before_packages() {
        // Regression: the udev rule + usermod steps must not be chained *after*
        // the package install with `&&` — a routine pacman mirror failure on a
        // fresh Arch/CachyOS system then aborts the permission setup and the
        // startup permissions warning reappears forever.
        let script = build_privileged_setup_script("pacman", "alice");

        let rule_pos = script.find("99-voxctrl.rules").expect("script writes udev rule");
        let usermod_pos = script.find("usermod -aG input alice").expect("script adds user to input group");
        let pkg_pos = script.find("pacman -S").expect("script installs packages");

        assert!(rule_pos < pkg_pos, "udev rule must be written before package install");
        assert!(usermod_pos < pkg_pos, "usermod must run before package install");
        // Package failure must not change the script's exit status.
        assert!(script.contains("||"), "package install must be best-effort");
    }

    #[test]
    fn test_privileged_script_grants_immediate_access() {
        // The whole point of the uaccess rule is that hotkeys work without the
        // user logging out. Reloading the rules is not enough on its own —
        // logind only re-applies the ACLs when the devices are re-triggered.
        let script = build_privileged_setup_script("apt", "alice");
        assert!(script.contains("TAG+=\"uaccess\""), "rule must grant session ACLs");
        assert!(script.contains("udevadm control --reload-rules"));
        assert!(
            script.contains("udevadm trigger --subsystem-match=input --action=change"),
            "input devices must be re-triggered so the new ACLs apply immediately"
        );

        let reload_pos = script.find("--reload-rules").unwrap();
        let trigger_pos = script.find("--subsystem-match=input").unwrap();
        assert!(reload_pos < trigger_pos, "rules must be reloaded before triggering");
    }

    #[test]
    fn test_privileged_script_only_fails_on_the_rule_write() {
        // Everything after the rule is best-effort: a missing `usermod`, a
        // udevadm quirk or a broken package mirror must not report the whole
        // setup as failed when hotkey access was actually configured.
        let script = build_privileged_setup_script("dnf", "alice");
        for step in ["udevadm control", "udevadm trigger", "usermod -aG input alice"] {
            let line = script
                .lines()
                .find(|l| l.starts_with(step))
                .unwrap_or_else(|| panic!("script runs `{step}`"));
            assert!(line.ends_with("|| true"), "`{step}` must be best-effort");
        }
        assert!(script.trim_end().ends_with("exit 0"));
    }

    #[test]
    fn test_privileged_script_without_packages_or_user() {
        let script = build_privileged_setup_script("unknown", "");
        assert!(script.contains("99-voxctrl.rules"));
        assert!(!script.contains("usermod"));
        assert!(!script.contains("apt-get install"));
    }

    #[test]
    fn test_rule_content_is_current_rejects_legacy_group_only_rule() {
        // 0.3.1 and earlier shipped a uinput-group-only rule, which is exactly
        // the rule that forces a logout. An upgrading user must be told to
        // re-run setup rather than being shown a green checkmark.
        let legacy = "KERNEL==\"uinput\", GROUP=\"input\", MODE=\"0660\", OPTIONS+=\"static_node=uinput\"";
        assert!(!rule_content_is_current(legacy));
        assert!(rule_content_is_current(UDEV_RULE_CONTENT));
    }

    #[test]
    fn test_manual_commands_cover_the_same_steps_as_the_script() {
        let manual = manual_setup_commands("apt", "alice");
        assert!(manual.contains(UDEV_RULE_PATH));
        assert!(manual.contains("TAG+=\"uaccess\""));
        assert!(manual.contains("udevadm control --reload-rules"));
        assert!(manual.contains("usermod -aG input alice"));
        assert!(manual.contains("apt-get install"));
    }

    #[test]
    fn test_relaunch_command_waits_for_the_old_instance() {
        // Launching while the old process is alive just hits the single-instance
        // guard, which focuses the existing window and exits — leaving the user
        // on the same broken instance they were trying to escape.
        let cmd = build_relaunch_command("/opt/VoxCtrl.AppImage", 4321);
        assert!(cmd.starts_with("while kill -0 4321"));
        assert!(cmd.contains("exec '/opt/VoxCtrl.AppImage'"));
    }

    #[test]
    fn test_relaunch_command_quotes_hostile_paths() {
        let cmd = build_relaunch_command("/home/o'brien/My Apps/VoxCtrl.AppImage", 7);
        assert!(cmd.contains(r"exec '/home/o'\''brien/My Apps/VoxCtrl.AppImage'"));
    }

    #[test]
    fn test_relaunch_is_not_offered_twice() {
        let _lock = crate::test_utils::get_env_lock().lock().unwrap();
        std::env::set_var("VOXCTRL_FAKE_COMMANDS", "sg");
        std::env::set_var("USER", "root"); // root is in no `input` group here
        std::env::set_var(RELAUNCH_MARKER_ENV, "1");
        assert!(
            !can_relaunch_with_input_group(),
            "a relaunched instance must never relaunch itself again"
        );
        std::env::remove_var(RELAUNCH_MARKER_ENV);
        std::env::remove_var("VOXCTRL_FAKE_COMMANDS");
        std::env::remove_var("USER");
    }

    #[test]
    fn test_relaunch_requires_sg() {
        let _lock = crate::test_utils::get_env_lock().lock().unwrap();
        std::env::remove_var(RELAUNCH_MARKER_ENV);
        std::env::set_var("VOXCTRL_FAKE_COMMANDS", "pkexec");
        assert!(!can_relaunch_with_input_group());
        std::env::remove_var("VOXCTRL_FAKE_COMMANDS");
    }

    #[test]
    fn test_detect_pkg_manager_mocked() {
        let _lock = crate::test_utils::get_env_lock().lock().unwrap();
        std::env::set_var("VOXCTRL_PKG_MANAGER_MOCK", "pacman");
        assert_eq!(detect_pkg_manager(), "pacman");

        std::env::set_var("VOXCTRL_PKG_MANAGER_MOCK", "apt");
        assert_eq!(detect_pkg_manager(), "apt");

        std::env::remove_var("VOXCTRL_PKG_MANAGER_MOCK");
    }

    #[test]
    fn test_setup_desktop_integration_success() {
        let _lock = crate::test_utils::get_env_lock().lock().unwrap();
        let temp_dir = tempdir().unwrap();
        let home_path = temp_dir.path().to_path_buf();
        
        // Mock HOME directory environment variable
        std::env::set_var("HOME", &home_path);
        // Mock APPIMAGE environment variable
        std::env::set_var("APPIMAGE", "/usr/bin/voxctrl-fake-appimage");

        let res = setup_desktop_integration();
        assert!(res.is_ok(), "desktop integration failed: {:?}", res);

        let desktop_file = home_path.join(".local/share/applications/voxctrl.desktop");
        let icon_file = home_path.join(".local/share/icons/hicolor/128x128/apps/voxctrl.png");

        assert!(desktop_file.exists(), "desktop file was not created");
        assert!(icon_file.exists(), "icon file was not created");

        let content = fs::read_to_string(desktop_file).unwrap();
        assert!(content.contains("Name=VoxCtrl"));
        assert!(content.contains("Exec=/usr/bin/voxctrl-fake-appimage"));
        assert!(content.contains("Icon=voxctrl"));

        std::env::remove_var("HOME");
        std::env::remove_var("APPIMAGE");
    }

    #[test]
    fn test_setup_desktop_integration_failure_readonly() {
        // Root can create directories anywhere, so the "unwritable path" premise
        // doesn't hold — skip (e.g. containerized CI). Panicking here would also
        // poison the shared env lock and cascade failures into unrelated tests.
        #[cfg(unix)]
        {
            let uid = std::process::Command::new("id").arg("-u").output()
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                .unwrap_or_default();
            if uid == "0" {
                return;
            }
        }

        let _lock = crate::test_utils::get_env_lock().lock().unwrap();
        // Set HOME to a non-existent/readonly path
        std::env::set_var("HOME", "/nonexistent_directory_voxctrl_test");
        let res = setup_desktop_integration();
        assert!(res.is_err(), "Expected failure when writing to nonexistent/readonly path");
        std::env::remove_var("HOME");
    }

    #[test]
    fn test_run_cli_installer_success() {
        let _lock = crate::test_utils::get_env_lock().lock().unwrap();
        std::env::set_var("VOXCTRL_PKG_MANAGER_MOCK", "apt");
        std::env::set_var("VOXCTRL_INSTALLER_TEST_MOCK", "success");
        
        let temp_dir = tempdir().unwrap();
        std::env::set_var("HOME", temp_dir.path());

        let res = run_cli_installer();
        assert!(res.is_ok(), "run_cli_installer failed: {:?}", res);

        std::env::remove_var("VOXCTRL_PKG_MANAGER_MOCK");
        std::env::remove_var("VOXCTRL_INSTALLER_TEST_MOCK");
        std::env::remove_var("HOME");
    }

    #[test]
    fn test_run_cli_installer_failure() {
        let _lock = crate::test_utils::get_env_lock().lock().unwrap();
        std::env::set_var("VOXCTRL_PKG_MANAGER_MOCK", "apt");
        std::env::set_var("VOXCTRL_INSTALLER_TEST_MOCK", "failure");
        
        let temp_dir = tempdir().unwrap();
        std::env::set_var("HOME", temp_dir.path());

        let res = run_cli_installer();
        assert!(res.is_err(), "Expected installer failure status");

        std::env::remove_var("VOXCTRL_PKG_MANAGER_MOCK");
        std::env::remove_var("VOXCTRL_INSTALLER_TEST_MOCK");
        std::env::remove_var("HOME");
    }

    #[tokio::test]
    async fn test_run_gui_installer_success() {
        let _lock = crate::test_utils::get_env_lock().lock().unwrap();
        std::env::set_var("VOXCTRL_PKG_MANAGER_MOCK", "apt");
        std::env::set_var("VOXCTRL_INSTALLER_TEST_MOCK", "success");
        std::env::set_var("VOXCTRL_FAKE_COMMANDS", "pkexec");

        let temp_dir = tempdir().unwrap();
        std::env::set_var("HOME", temp_dir.path());

        let res = run_gui_installer().await;
        assert!(res.is_ok(), "run_gui_installer failed: {:?}", res);

        std::env::remove_var("VOXCTRL_PKG_MANAGER_MOCK");
        std::env::remove_var("VOXCTRL_INSTALLER_TEST_MOCK");
        std::env::remove_var("VOXCTRL_FAKE_COMMANDS");
        std::env::remove_var("HOME");
    }

    #[tokio::test]
    async fn test_run_gui_installer_failure() {
        let _lock = crate::test_utils::get_env_lock().lock().unwrap();
        std::env::set_var("VOXCTRL_PKG_MANAGER_MOCK", "apt");
        std::env::set_var("VOXCTRL_INSTALLER_TEST_MOCK", "failure");
        std::env::set_var("VOXCTRL_FAKE_COMMANDS", "pkexec");

        let temp_dir = tempdir().unwrap();
        std::env::set_var("HOME", temp_dir.path());

        let res = run_gui_installer().await;
        assert!(res.is_err(), "Expected GUI installer failure status");

        std::env::remove_var("VOXCTRL_PKG_MANAGER_MOCK");
        std::env::remove_var("VOXCTRL_INSTALLER_TEST_MOCK");
        std::env::remove_var("VOXCTRL_FAKE_COMMANDS");
        std::env::remove_var("HOME");
    }

    #[tokio::test]
    async fn test_run_gui_installer_without_pkexec_explains_itself() {
        // Without polkit the one-click setup cannot run at all. Failing with a
        // bare "setup failed" leaves the user with no way forward, so the error
        // has to point at the manual commands the setup window shows.
        let _lock = crate::test_utils::get_env_lock().lock().unwrap();
        std::env::set_var("VOXCTRL_PKG_MANAGER_MOCK", "apt");
        std::env::set_var("VOXCTRL_INSTALLER_TEST_MOCK", "success");
        std::env::set_var("VOXCTRL_FAKE_COMMANDS", "");

        let err = run_gui_installer().await.unwrap_err();
        assert!(err.contains("pkexec"), "error must name the missing tool: {err}");
        assert!(err.contains("manually"), "error must offer a way forward: {err}");

        std::env::remove_var("VOXCTRL_PKG_MANAGER_MOCK");
        std::env::remove_var("VOXCTRL_INSTALLER_TEST_MOCK");
        std::env::remove_var("VOXCTRL_FAKE_COMMANDS");
    }
}

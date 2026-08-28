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

/// Builds the privileged setup script.
///
/// This installs host packages only — the keystroke-injection helpers and
/// runtime libraries VoxCtrl needs to type a transcription into the focused
/// window. It grants no access to input devices, and deliberately so.
///
/// VoxCtrl used to write a udev rule here that tagged every `/dev/input/event*`
/// node with `uaccess`, plus `usermod -aG input`. Both made this process able to
/// read the keyboard — and with it every other process running as the user,
/// permanently, for every application on the machine. systemd's own defaults
/// grant `uaccess` on input devices to joysticks and nothing else, precisely to
/// keep unprivileged programs from reading keystrokes. Global shortcuts now go
/// through the desktop portal, which needs none of that, so the rule is gone
/// and is never written again.
pub fn build_privileged_setup_script(pkg_mgr: &str) -> String {
    let mut script = String::from("set -u\n");
    if let Some(cmd) = get_install_packages_command(pkg_mgr) {
        script.push_str(&format!(
            "{{ {cmd}; }} || echo 'VoxCtrl: host package installation failed (mirrors/network?). Install the packages manually later.' >&2\n"
        ));
    }
    script.push_str("exit 0\n");
    script
}

/// The commands a user can run by hand if the graphical setup cannot run
/// (no pkexec, no polkit agent, locked-down machine).
pub fn manual_setup_commands(pkg_mgr: &str) -> String {
    match get_install_packages_command(pkg_mgr) {
        Some(cmd) => format!("sudo sh -c '{cmd}'"),
        None => String::new(),
    }
}

pub fn command_exists(cmd: &str) -> bool {
    #[cfg(test)]
    {
        if let Ok(list) = std::env::var("VOXCTRL_FAKE_COMMANDS") {
            return list.split(',').any(|c| c == cmd);
        }
    }
    voxctrl_config::find_in_path(cmd).is_some()
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

/// Desktop entry filename. Must stay in step with the portal application id, so
/// the desktop can name and illustrate the shortcuts VoxCtrl registers.
pub const DESKTOP_FILE_NAME: &str = "ai.voxctrl.app.desktop";

/// What the entry was called before it had to match the application id.
pub const LEGACY_DESKTOP_FILE_NAME: &str = "voxctrl.desktop";

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

    // Named after the application id VoxCtrl declares to the desktop portal
    // (`voxctrl_hotkeys::portal::APP_ID`). That is how a desktop resolves the id
    // it is handed for global shortcuts back to a name and icon — without the
    // match, KDE's shortcut settings list a bare identifier.
    let launcher_path = launcher_dir.join(DESKTOP_FILE_NAME);
    std::fs::write(&launcher_path, desktop_content).map_err(|e| e.to_string())?;

    // An install from before the rename leaves a second entry behind, which
    // shows up as a duplicate VoxCtrl in the application menu.
    let legacy = launcher_dir.join(LEGACY_DESKTOP_FILE_NAME);
    if legacy.exists() {
        let _ = std::fs::remove_file(&legacy);
    }
    
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

    #[cfg(target_os = "linux")]
    {
        if crate::mint_shortcuts::is_mint_desktop() && !crate::mint_shortcuts::is_mint_shortcut_registered() {
            let _ = crate::mint_shortcuts::register_mint_shortcut(None);
        }
    }

    Ok(())
}

pub fn run_cli_installer() -> Result<(), String> {
    println!("=== VoxCtrl CLI Installer & Host Setup ===");
    let pkg_mgr = detect_pkg_manager();
    println!("Detected Package Manager: {}", pkg_mgr);

    let full_script = build_privileged_setup_script(pkg_mgr);
    println!("Preparing to run system setup command via sudo...");
    println!("Executing: sudo sh -c \"{}\"", full_script);

    let status = run_command_status("sudo", &["sh", "-c", &full_script])?;

    if !status.success() {
        return Err("Host package installation failed.".to_string());
    }

    println!("System dependencies installed successfully!");

    // Setup desktop integration
    println!("Registering desktop entry and icon...");
    setup_desktop_integration()?;
    println!("Desktop integration complete!");

    println!("\n==================================================");
    println!("  Setup & Integration Complete!");
    println!("==================================================");

    println!();
    println!("Global shortcuts are registered with your desktop through the XDG");
    println!("desktop portal, so there is nothing else to grant and no keyboard");
    println!("access to set up. Start VoxCtrl; it reports in-app if your desktop");
    println!("does not provide the portal.");
    Ok(())
}

pub async fn run_gui_installer() -> Result<(), String> {
    let pkg_mgr = detect_pkg_manager();
    let full_script = build_privileged_setup_script(pkg_mgr);

    if get_install_packages_command(pkg_mgr).is_none() {
        // Nothing to install on this distro; the desktop entry is all that is
        // left, and it needs no privileges.
        return setup_desktop_integration();
    }

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
        return Err("Installing the host packages failed or was canceled.".to_string());
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
    fn the_privileged_script_never_touches_input_permissions() {
        // The whole point of the change: VoxCtrl asks for administrator rights
        // to install packages and for nothing else. A udev rule tagging input
        // devices with `uaccess`, or `usermod -aG input`, would let every
        // process running as this user read every keystroke on the machine.
        for mgr in ["pacman", "apt", "dnf", "zypper", "unknown"] {
            let script = build_privileged_setup_script(mgr);
            for forbidden in [
                "udev",
                "uaccess",
                "usermod",
                "/dev/input",
                "99-voxctrl.rules",
                "udevadm",
            ] {
                assert!(
                    !script.contains(forbidden),
                    "{mgr} setup script must not mention `{forbidden}`:\n{script}"
                );
            }
        }
    }

    #[test]
    fn the_privileged_script_installs_packages_best_effort() {
        // A stale mirror is routine on rolling distros and must not read as a
        // failed setup — nothing else in the script depends on it.
        let script = build_privileged_setup_script("pacman");
        assert!(script.contains("pacman -S"));
        assert!(script.contains("||"), "package install must be best-effort");
        assert!(script.trim_end().ends_with("exit 0"));
    }

    #[test]
    fn the_privileged_script_is_empty_work_on_an_unknown_distro() {
        let script = build_privileged_setup_script("unknown");
        assert!(!script.contains("install"));
        assert!(script.trim_end().ends_with("exit 0"));
    }

    #[test]
    fn manual_commands_cover_the_packages_and_nothing_privileged() {
        let manual = manual_setup_commands("apt");
        assert!(manual.contains("apt-get install"));
        for forbidden in ["udev", "usermod", "uaccess", "/dev/input"] {
            assert!(
                !manual.contains(forbidden),
                "manual commands must not mention `{forbidden}`"
            );
        }
        assert!(manual_setup_commands("unknown").is_empty());
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

        let desktop_file = home_path.join(".local/share/applications/ai.voxctrl.app.desktop");
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

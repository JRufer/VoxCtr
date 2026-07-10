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

/// Builds the privileged setup script shared by the CLI and GUI installers.
///
/// The hotkey-permission steps (udev rule, rule reload, input-group membership)
/// come FIRST and determine the script's exit status. The host package
/// installation runs last as best-effort: on rolling distros (Arch/CachyOS) a
/// stale mirror or out-of-date sync database makes the package step fail
/// routinely, and that must not abort the permission setup — otherwise the
/// startup permissions warning reappears forever even though the user ran the
/// setup as instructed.
pub fn build_privileged_setup_script(pkg_mgr: &str, username: &str) -> String {
    let mut commands = vec![
        "echo 'KERNEL==\"uinput\", GROUP=\"input\", MODE=\"0660\", OPTIONS+=\"static_node=uinput\"' > /etc/udev/rules.d/99-voxctrl.rules".to_string(),
        "udevadm control --reload-rules && udevadm trigger".to_string(),
    ];
    if !username.is_empty() {
        commands.push(format!("usermod -aG input {}", username));
    }

    let mut script = commands.join(" && ");
    if let Some(cmd) = get_install_packages_command(pkg_mgr) {
        script.push_str(&format!(
            " && ({{ {cmd}; }} || echo 'VoxCtrl: host package installation failed (mirrors/network?). Hotkey permissions were still configured; install the packages manually later.' >&2)"
        ));
    }
    script
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
    println!("Please log out and log back in (or reboot) for evdev hotkeys to work.");
    Ok(())
}

pub async fn run_gui_installer() -> Result<(), String> {
    let pkg_mgr = detect_pkg_manager();
    let username = std::env::var("USER").unwrap_or_default();
    let full_script = build_privileged_setup_script(pkg_mgr, &username);

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
    fn test_privileged_script_without_packages_or_user() {
        let script = build_privileged_setup_script("unknown", "");
        assert!(script.contains("99-voxctrl.rules"));
        assert!(!script.contains("usermod"));
        assert!(!script.contains("install"));
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
        
        let temp_dir = tempdir().unwrap();
        std::env::set_var("HOME", temp_dir.path());

        let res = run_gui_installer().await;
        assert!(res.is_ok(), "run_gui_installer failed: {:?}", res);

        std::env::remove_var("VOXCTRL_PKG_MANAGER_MOCK");
        std::env::remove_var("VOXCTRL_INSTALLER_TEST_MOCK");
        std::env::remove_var("HOME");
    }

    #[tokio::test]
    async fn test_run_gui_installer_failure() {
        let _lock = crate::test_utils::get_env_lock().lock().unwrap();
        std::env::set_var("VOXCTRL_PKG_MANAGER_MOCK", "apt");
        std::env::set_var("VOXCTRL_INSTALLER_TEST_MOCK", "failure");
        
        let temp_dir = tempdir().unwrap();
        std::env::set_var("HOME", temp_dir.path());

        let res = run_gui_installer().await;
        assert!(res.is_err(), "Expected GUI installer failure status");

        std::env::remove_var("VOXCTRL_PKG_MANAGER_MOCK");
        std::env::remove_var("VOXCTRL_INSTALLER_TEST_MOCK");
        std::env::remove_var("HOME");
    }
}

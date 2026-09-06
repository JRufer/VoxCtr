use std::path::PathBuf;

pub fn get_overlay_path() -> Option<PathBuf> {
    // Logged via tracing (stderr) so the resolved path is visible even when
    // stdout is block-buffered behind a pipe.
    if let Ok(mut current_path) = std::env::current_exe() {
        tracing::info!("get_overlay_path: current_exe = {:?}", current_path);
        current_path.pop(); // Pop binary name
        let bin_name = if cfg!(target_os = "windows") {
            "voxctrl-overlay.exe"
        } else {
            "voxctrl-overlay"
        };
        // Check same dir (where the bundled sidecar lives next to the main app)
        let p1 = current_path.join(bin_name);
        if p1.exists() {
            tracing::info!("get_overlay_path: found alongside main binary: {:?}", p1);
            return Some(p1);
        }
        // Check parent dir (if running in deps/)
        current_path.pop();
        let p2 = current_path.join(bin_name);
        if p2.exists() {
            tracing::info!("get_overlay_path: found in parent dir: {:?}", p2);
            return Some(p2);
        }

        // Dev-only fallback: relative to the current working directory.
        if let Ok(cwd) = std::env::current_dir() {
            let p3 = cwd.join("target").join("debug").join(bin_name);
            if p3.exists() {
                tracing::info!("get_overlay_path: found in target/debug: {:?}", p3);
                return Some(p3);
            }
            let p4 = cwd.join("src-tauri").join("target").join("debug").join(bin_name);
            if p4.exists() {
                tracing::info!("get_overlay_path: found in src-tauri/target/debug: {:?}", p4);
                return Some(p4);
            }
        }
    }
    tracing::error!("get_overlay_path: overlay binary not found");
    None
}

pub fn spawn_overlay_process(overlay_rx: crossbeam_channel::Receiver<String>) {
    if let Some(overlay_path) = get_overlay_path() {
        tracing::info!("Spawning Slint overlay helper: {:?}", overlay_path);
        let mut overlay_cmd = std::process::Command::new(&overlay_path);
        overlay_cmd
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit());

        // CREATE_NO_WINDOW. The overlay is a GUI process, but it is started by
        // one, and without this Windows still hands the child a console — which
        // appears as a black rectangle beside the overlay on the first
        // dictation.
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            overlay_cmd.creation_flags(CREATE_NO_WINDOW);
        }
        // Run the overlay through XWayland on Wayland sessions. A native
        // Wayland client cannot control its own stacking or position, so
        // always-on-top and the configured placement are simply ignored
        // by the compositor. Under XWayland the overlay is an X11 client
        // and KWin honors _NET_WM_STATE_ABOVE and window positioning.
        // Only when an X server (XWayland) is actually reachable; on a
        // pure X11 session WAYLAND_DISPLAY is unset and this is a no-op.
        if std::env::var_os("DISPLAY").is_some()
            && std::env::var_os("WAYLAND_DISPLAY").is_some()
        {
            tracing::info!("Wayland detected; running overlay via XWayland for always-on-top + positioning");
            overlay_cmd.env_remove("WAYLAND_DISPLAY");
            overlay_cmd.env_remove("WAYLAND_SOCKET");
        }
        match overlay_cmd.spawn() {
            Ok(mut child) => {
                if let Some(mut stdin) = child.stdin.take() {
                    let rx = overlay_rx.clone();
                    std::thread::spawn(move || {
                        use std::io::Write;
                        while let Ok(msg) = rx.recv() {
                            if writeln!(stdin, "{}", msg).is_err() || stdin.flush().is_err() {
                                break;
                            }
                        }
                    });
                }
                // Wait for child in background so it doesn't become a zombie
                std::thread::spawn(move || {
                    let status = child.wait();
                    // Logged on stderr (tracing) so it is visible even when
                    // stdout is block-buffered behind a pipe. If this fires
                    // after the first dictation, the overlay is crashing
                    // rather than hitting a window-mapping issue.
                    tracing::warn!("Slint overlay process exited: {:?}", status);
                });
            }
            Err(e) => {
                eprintln!("Failed to spawn Slint overlay process: {:?}", e);
            }
        }
    } else {
        eprintln!("Slint overlay binary not found! Check your build directory.");
    }
}

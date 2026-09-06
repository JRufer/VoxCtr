use anyhow::Result;
use tracing::debug;
#[cfg(target_os = "linux")]
use tracing::warn;


/// Inject text into the currently focused window using the best available
/// method for the current platform and display server.
pub async fn inject_text(text: &str) -> Result<()> {
    #[cfg(target_os = "linux")]
    return inject_linux(text).await;

    #[cfg(target_os = "windows")]
    return inject_windows(text).await;

    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    anyhow::bail!("Text injection not supported on this platform");
}

// ── Linux ─────────────────────────────────────────────────────────────────────

#[cfg(target_os = "linux")]
async fn inject_linux(text: &str) -> Result<()> {
    let wayland = std::env::var("WAYLAND_DISPLAY").is_ok();

    // 1. wtype (Wayland native)
    if wayland && voxctrl_config::find_in_path("wtype").is_some() {
        if run_cmd("wtype", &["--", text]).await {
            debug!("Injected via wtype");
            return Ok(());
        }
        warn!("wtype failed; trying fallback");
    }

    // 2. xdotool (X11 / XWayland)
    if voxctrl_config::find_in_path("xdotool").is_some() {
        if run_cmd("xdotool", &["type", "--clearmodifiers", "--delay", "12", "--", text]).await {
            debug!("Injected via xdotool");
            return Ok(());
        }
        warn!("xdotool failed; trying clipboard fallback");
    }

    // 3. Clipboard + paste (last resort)
    clipboard_paste(text).await?;
    Ok(())
}

#[cfg(target_os = "linux")]
async fn run_cmd(bin: &str, args: &[&str]) -> bool {
    tokio::process::Command::new(bin)
        .args(args)
        .status()
        .await
        .map(|s| s.success())
        .unwrap_or(false)
}

#[cfg(target_os = "linux")]
async fn clipboard_paste(text: &str) -> Result<()> {
    // Copy to clipboard, then simulate Ctrl+V
    let t = text.to_string();
    tokio::task::spawn_blocking(move || {
        let mut cb = arboard::Clipboard::new()?;
        cb.set_text(&t)?;
        anyhow::Ok(())
    })
    .await??;

    if std::env::var("WAYLAND_DISPLAY").is_ok() && voxctrl_config::find_in_path("wtype").is_some() {
        run_cmd("wtype", &["-M", "ctrl", "v", "-m", "ctrl"]).await;
    } else if voxctrl_config::find_in_path("xdotool").is_some() {
        run_cmd("xdotool", &["key", "--clearmodifiers", "ctrl+v"]).await;
    }
    Ok(())
}

// ── Windows ───────────────────────────────────────────────────────────────────

#[cfg(target_os = "windows")]
async fn inject_windows(text: &str) -> Result<()> {
    // `SendInput` is a blocking Win32 call and long text is a lot of events, so
    // it does not belong on an async worker.
    let t = text.to_string();
    tokio::task::spawn_blocking(move || voxctrl_winput::deliver(&t)).await??;
    debug!("Injected via SendInput");
    Ok(())
}

// ── Desktop notifications ─────────────────────────────────────────────────────

pub fn show_notification(summary: &str, body: &str) {
    let summary = summary.to_string();
    let body = body.to_string();
    // Fire-and-forget; don't block the caller
    std::thread::spawn(move || {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            let _ = notify_rust::Notification::new()
                .summary(&summary)
                .body(&body)
                .timeout(notify_rust::Timeout::Milliseconds(3000))
                .show();
        }
        #[cfg(target_os = "windows")]
        {
            let _ = notify_rust::Notification::new()
                .summary(&summary)
                .body(&body)
                .show();
        }
    });
}

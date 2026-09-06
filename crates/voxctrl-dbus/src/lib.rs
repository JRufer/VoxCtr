use std::sync::Arc;

use anyhow::Result;
use tokio::sync::Mutex;
#[cfg(target_os = "linux")]
use tracing::info;

// ── Shared state ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DictationStatus {
    Idle,
    Recording,
    Transcribing,
}

impl std::fmt::Display for DictationStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DictationStatus::Idle => write!(f, "idle"),
            DictationStatus::Recording => write!(f, "recording"),
            DictationStatus::Transcribing => write!(f, "transcribing"),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct AppState {
    pub status: DictationStatus,
    pub word_count: u32,
}

impl Default for DictationStatus {
    fn default() -> Self {
        Self::Idle
    }
}

// ── DBus service (Linux only) ────────────────────────────────────────────────

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use zbus::{interface, Connection, SignalContext};

    pub struct DictationInterface {
        pub state: Arc<Mutex<AppState>>,
        /// Channels to send control commands back to the app coordinator. The
        /// start channel carries the id of the binding that asked for it, so a
        /// desktop shortcut can dictate into that binding's own targets. Empty
        /// means "whichever binding the app would use by default".
        pub start_tx: tokio::sync::mpsc::Sender<String>,
        pub stop_tx: tokio::sync::mpsc::Sender<()>,
    }

    #[interface(name = "ai.voxctrl.Dictation")]
    impl DictationInterface {
        async fn start_recording(&self) -> zbus::fdo::Result<()> {
            let _ = self.start_tx.send(String::new()).await;
            Ok(())
        }

        async fn stop_recording(&self) -> zbus::fdo::Result<()> {
            let _ = self.stop_tx.send(()).await;
            Ok(())
        }

        async fn toggle_recording(&self) -> zbus::fdo::Result<()> {
            self.toggle(String::new()).await
        }

        /// Toggle dictation for one specific binding.
        ///
        /// This is what a Cinnamon/MATE native shortcut calls: the desktop owns
        /// the key grab there, so the only way VoxCtrl learns *which* of the
        /// user's bindings fired — and therefore which targets the text goes to
        /// — is for the shortcut to name it.
        async fn toggle_binding(&self, binding_id: String) -> zbus::fdo::Result<()> {
            self.toggle(binding_id).await
        }

        async fn get_status(&self) -> zbus::fdo::Result<String> {
            let guard = self.state.lock().await;
            Ok(guard.status.to_string())
        }

        async fn get_word_count(&self) -> zbus::fdo::Result<u32> {
            let guard = self.state.lock().await;
            Ok(guard.word_count)
        }

        #[zbus(signal)]
        async fn status_changed(ctx: &SignalContext<'_>, status: &str) -> zbus::Result<()>;

        #[zbus(signal)]
        async fn text_injected(ctx: &SignalContext<'_>, text: &str) -> zbus::Result<()>;
    }

    impl DictationInterface {
        /// Start `binding_id` if idle, stop whatever is running if not.
        async fn toggle(&self, binding_id: String) -> zbus::fdo::Result<()> {
            let status = {
                let guard = self.state.lock().await;
                guard.status.clone()
            };
            if status == DictationStatus::Recording {
                let _ = self.stop_tx.send(()).await;
            } else {
                let _ = self.start_tx.send(binding_id).await;
            }
            Ok(())
        }
    }

    pub async fn start_service(
        state: Arc<Mutex<AppState>>,
        start_tx: tokio::sync::mpsc::Sender<String>,
        stop_tx: tokio::sync::mpsc::Sender<()>,
    ) -> Result<Connection> {
        let iface = DictationInterface {
            state,
            start_tx,
            stop_tx,
        };

        let conn = Connection::session().await?;
        conn.object_server()
            .at("/ai/voxctrl/Dictation", iface)
            .await?;
        conn.request_name("ai.voxctrl.Dictation").await?;
        info!("DBus service registered: ai.voxctrl.Dictation");
        Ok(conn)
    }

    pub async fn emit_status_changed(
        conn: &Connection,
        status: &str,
    ) -> Result<()> {
        let iface_ref = conn
            .object_server()
            .interface::<_, DictationInterface>("/ai/voxctrl/Dictation")
            .await?;
        DictationInterface::status_changed(iface_ref.signal_context(), status).await?;
        Ok(())
    }

    pub async fn emit_text_injected(conn: &Connection, text: &str) -> Result<()> {
        let iface_ref = conn
            .object_server()
            .interface::<_, DictationInterface>("/ai/voxctrl/Dictation")
            .await?;
        DictationInterface::text_injected(iface_ref.signal_context(), text).await?;
        Ok(())
    }
}

#[cfg(target_os = "linux")]
pub use linux::{emit_status_changed, emit_text_injected, start_service};

// ── Stub for non-Linux platforms ──────────────────────────────────────────────

#[cfg(not(target_os = "linux"))]
pub async fn start_service(
    _state: Arc<Mutex<AppState>>,
    _start_tx: tokio::sync::mpsc::Sender<String>,
    _stop_tx: tokio::sync::mpsc::Sender<()>,
) -> Result<()> {
    tracing::warn!("DBus service not available on this platform");
    Ok(())
}

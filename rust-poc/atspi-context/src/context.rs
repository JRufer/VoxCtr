//! AT-SPI2 focus tracker, context reader, and direct text injector.
//!
//! Rust port of `src/atspi_context.py` from VoxCtr.
//! Public surface mirrors the Python module:
//!   - `FocusTracker::start()` spawns the event listener.
//!   - `FocusTracker::get_focused_context()` returns a [`FocusContext`] snapshot.
//!   - `FocusTracker::inject_text()` inserts text at the caret via EditableText.

use std::sync::Arc;

use anyhow::{Context, Result};
use atspi::events::object::StateChangedEvent;
use atspi::events::{Event, ObjectEvents};
use atspi::proxy::accessible::{AccessibleProxy, ObjectRefExt};
use atspi::proxy::editable_text::EditableTextProxy;
use atspi::proxy::text::TextProxy;
use atspi::zbus;
use atspi::{AccessibilityConnection, ObjectRefOwned, State};
use futures_lite::stream::StreamExt;
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

/// Roles that suggest a code/terminal context (matches the Python set).
const CODE_ROLES: &[&str] = &["terminal", "text"];

/// Snapshot of the focused accessible widget at a point in time.
#[derive(Debug, Clone)]
pub struct FocusContext {
    pub app_name: String,
    pub role_name: String,
    /// Text before the caret, up to `max_chars` characters.
    pub surrounding_text: String,
    pub cursor_offset: i32,
    /// True when the role looks like a terminal / IDE editor.
    pub is_code_context: bool,
}

/// Tracks the last-focused accessible widget across the AT-SPI2 bus.
///
/// Cheap to clone — internally an Arc.
#[derive(Clone)]
pub struct FocusTracker {
    inner: Arc<Inner>,
}

struct Inner {
    conn: AccessibilityConnection,
    focused: Mutex<Option<ObjectRefOwned>>,
}

impl FocusTracker {
    /// Connect to the AT-SPI2 bus. Returns an error if the bus is not running.
    pub async fn connect() -> Result<Self> {
        let conn = AccessibilityConnection::new()
            .await
            .context("failed to connect to AT-SPI2 bus (is at-spi2-registryd running?)")?;
        Ok(Self {
            inner: Arc::new(Inner {
                conn,
                focused: Mutex::new(None),
            }),
        })
    }

    /// Register the state-changed match rule and spawn the event loop.
    ///
    /// Idempotent at the AT-SPI level — calling twice produces two listeners,
    /// so callers should only call this once per process.
    pub async fn start(&self) -> Result<()> {
        self.inner
            .conn
            .register_event::<StateChangedEvent>()
            .await
            .context("register_event(StateChangedEvent) failed")?;

        let this = self.clone();
        tokio::spawn(async move {
            let mut stream = this.inner.conn.event_stream();
            while let Some(item) = stream.next().await {
                match item {
                    Ok(Event::Object(ObjectEvents::StateChanged(ev))) => {
                        if ev.state == State::Focused && ev.enabled {
                            let mut guard = this.inner.focused.lock().await;
                            *guard = Some(ev.item.clone());
                            debug!(
                                "focus gained: {:?} @ {:?}",
                                ev.item.name(),
                                ev.item.path()
                            );
                        }
                    }
                    Ok(_) => {}
                    Err(e) => warn!("event stream error: {e}"),
                }
            }
            info!("AT-SPI2 event stream ended");
        });

        info!("AT-SPI2 focus tracker started");
        Ok(())
    }

    /// Currently tracked focused widget, or `None` if nothing has been seen.
    pub async fn focused_ref(&self) -> Option<ObjectRefOwned> {
        self.inner.focused.lock().await.clone()
    }

    /// Snapshot of the focused widget, equivalent to Python's
    /// `get_focused_context(max_chars)`.
    pub async fn get_focused_context(&self, max_chars: i32) -> Option<FocusContext> {
        let obj = self.focused_ref().await?;
        match self.build_context(&obj, max_chars).await {
            Ok(ctx) => Some(ctx),
            Err(e) => {
                debug!("get_focused_context error: {e}");
                None
            }
        }
    }

    async fn build_context(
        &self,
        obj: &ObjectRefOwned,
        max_chars: i32,
    ) -> Result<FocusContext> {
        let zconn = self.inner.conn.connection();
        let accessible = obj
            .as_accessible_proxy(zconn)
            .await
            .context("as_accessible_proxy failed")?;

        let app_name = safe_app_name(&accessible).await;
        let role_name = accessible.get_role_name().await.unwrap_or_default();
        let (surrounding_text, cursor_offset) =
            safe_surrounding_text(zconn, obj, max_chars).await;
        let is_code_context = CODE_ROLES.iter().any(|r| *r == role_name);

        Ok(FocusContext {
            app_name,
            role_name,
            surrounding_text,
            cursor_offset,
            is_code_context,
        })
    }

    /// Insert `text` at the current caret position via the EditableText
    /// interface. Returns `Ok(true)` on success, `Ok(false)` when the widget
    /// does not expose EditableText. Errors are logged and surfaced.
    pub async fn inject_text(&self, text: &str) -> Result<bool> {
        if text.is_empty() {
            return Ok(false);
        }
        let Some(obj) = self.focused_ref().await else {
            return Ok(false);
        };
        let zconn = self.inner.conn.connection();

        // Caret offset comes from the Text interface.
        let text_proxy = build_text_proxy(zconn, &obj).await?;
        let offset = text_proxy
            .caret_offset()
            .await
            .context("read caret_offset failed")?;

        let editable = build_editable_text_proxy(zconn, &obj).await?;
        let length = text.chars().count() as i32;
        let ok = editable
            .insert_text(offset, text, length)
            .await
            .context("EditableText.insert_text failed")?;
        if ok {
            debug!("AT-SPI2 injected {} chars at offset {}", length, offset);
        }
        Ok(ok)
    }
}

// ── helpers ────────────────────────────────────────────────────────────────

async fn safe_app_name(accessible: &AccessibleProxy<'_>) -> String {
    match accessible.get_application().await {
        Ok(app_ref) => {
            // The application ObjectRef points at the app's root accessible;
            // its `name` property is the app name (e.g. "gedit", "firefox").
            match app_ref.as_accessible_proxy(accessible.inner().connection()).await {
                Ok(p) => p.name().await.unwrap_or_default(),
                Err(_) => String::new(),
            }
        }
        Err(_) => String::new(),
    }
}

async fn safe_surrounding_text(
    zconn: &zbus::Connection,
    obj: &ObjectRefOwned,
    max_chars: i32,
) -> (String, i32) {
    let Ok(text_proxy) = build_text_proxy(zconn, obj).await else {
        return (String::new(), 0);
    };
    let Ok(offset) = text_proxy.caret_offset().await else {
        return (String::new(), 0);
    };
    let start = (offset - max_chars).max(0);
    let body = text_proxy
        .get_text(start, offset)
        .await
        .unwrap_or_default();
    (body, offset)
}

async fn build_text_proxy<'a>(
    zconn: &'a zbus::Connection,
    obj: &'a ObjectRefOwned,
) -> Result<TextProxy<'a>> {
    let name = obj
        .name()
        .context("ObjectRef has no bus name")?
        .clone();
    let path = obj.path().clone();
    TextProxy::builder(zconn)
        .destination(zbus::names::BusName::from(name))?
        .path(path)?
        .cache_properties(zbus::proxy::CacheProperties::No)
        .build()
        .await
        .context("build TextProxy failed")
}

async fn build_editable_text_proxy<'a>(
    zconn: &'a zbus::Connection,
    obj: &'a ObjectRefOwned,
) -> Result<EditableTextProxy<'a>> {
    let name = obj
        .name()
        .context("ObjectRef has no bus name")?
        .clone();
    let path = obj.path().clone();
    EditableTextProxy::builder(zconn)
        .destination(zbus::names::BusName::from(name))?
        .path(path)?
        .cache_properties(zbus::proxy::CacheProperties::No)
        .build()
        .await
        .context("build EditableTextProxy failed")
}

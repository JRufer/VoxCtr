# atspi-context — Rust port PoC

A proof-of-concept Rust replacement for `src/atspi_context.py` from VoxCtr.

It validates that the [`atspi`](https://crates.io/crates/atspi) crate (v0.30,
pure-Rust on top of `zbus`) can deliver the three capabilities the Python
module exposes:

| Capability                         | Python (`atspi_context.py`)       | Rust (this PoC)                                    |
|------------------------------------|-----------------------------------|----------------------------------------------------|
| Detect the focused widget          | `Registry.registerEventListener("object:state-changed:focused")` | `register_event::<StateChangedEvent>()` + `event_stream()` |
| Get role / app / surrounding text  | `getApplication`, `getRoleName`, `queryText().getText(...)`       | `AccessibleProxy::get_application` / `get_role_name`, `TextProxy::get_text` |
| Insert at the caret                | `queryText().insertText(...)`     | `EditableTextProxy::insert_text(...)`              |
| Read cursor offset                 | `Text.caretOffset`                | `TextProxy::caret_offset()`                        |

The public API mirrors the Python module so that swapping the implementation
later is mechanical:

```rust
let tracker = FocusTracker::connect().await?;
tracker.start().await?;
let ctx = tracker.get_focused_context(500).await; // Option<FocusContext>
let ok  = tracker.inject_text("hello ").await?;   // bool
```

## Build

```
cargo build --release
```

Dependencies are pure-Rust (`atspi` → `zbus`). No `libatspi`, no GLib, no Python.

## Running on a real desktop

This PoC requires an active AT-SPI2 registry on the session bus. That is the
default on GNOME and most Wayland desktops; on others enable it with:

```
gsettings set org.gnome.desktop.interface toolkit-accessibility true
# or, for Qt apps:
export QT_ACCESSIBILITY=1
export QT_LINUX_ACCESSIBILITY_ALWAYS_ON=1
```

Then run any of:

```
./target/release/atspi-context watch              # stream focus changes
./target/release/atspi-context context 500        # snapshot the focused widget
./target/release/atspi-context inject "hello "    # insert at caret after 3s
./target/release/atspi-context demo               # full round trip
```

Set `RUST_LOG=debug` for verbose output:

```
RUST_LOG=debug ./target/release/atspi-context watch
```

## What is verified

Compilation and `clippy` both pass; the binary launches and produces a clear
error when no AT-SPI bus is present, confirming the connect / start / event
loop wiring is sound. The remaining verification — that focus events fire,
text is read, and `insert_text` lands — must be done interactively on a real
GNOME / KDE / sway+a11y session, since AT-SPI2 has no headless test harness.

## Findings vs. the Python implementation

* **Feature parity is achievable.** Every public symbol in `atspi_context.py`
  has a direct counterpart in the `atspi` crate. No capability is missing.
* **The Rust API is lower-level.** `pyatspi` lets you do
  `accessible.queryText()` and get back something that auto-resolves the
  underlying interface. In Rust you build a `TextProxy` (or
  `EditableTextProxy`) yourself by passing the same bus name + path.
  Helper functions (`build_text_proxy`, `build_editable_text_proxy`) hide
  this; if we port for real we'd promote them to a `Widget` wrapper that
  carries all three proxies together.
* **Async, not threaded.** The Python version uses a daemon thread and a
  GLib loop. This PoC uses a single tokio task on the event stream — fewer
  moving parts and no GLib dependency.
* **`get_application` returns an `ObjectRef`, not a string.** One extra
  proxy hop is needed to read `.name` for the app label. Cost is negligible
  (already done in Python via two D-Bus round trips).

## Open questions for the full port

1. **Application name lookup.** Confirm `obj.get_application()` returns the
   app root and that `.name` matches what GNOME and Qt apps publish (it
   should — same protocol — but worth verifying against the same apps the
   Python code is tested with).
2. **State change ordering.** The Python code only checks `event.detail1`
   (gained focus). The Rust code checks `state == Focused && enabled`.
   Need to verify enable/disable timing matches.
3. **AT-SPI bus auto-launch.** On some distros the bus is on-demand. The
   Python code degrades silently when pyatspi import fails; the Rust code
   fails the `connect()` call. We may want a `try_connect_or_disabled()`
   helper that mirrors the Python `is_available()` semantics.

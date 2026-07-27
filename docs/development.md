# Development Guide

## Prerequisites

### Required Tools
- **Rust** 1.75+ with `cargo` — [rustup.rs](https://rustup.rs/)
- **Node.js** 18+ with `npm`
- **Tauri CLI** 2.x — `cargo install tauri-cli`

### Linux System Dependencies
```bash
# Ubuntu / Debian
sudo apt install \
  libwebkit2gtk-4.1-dev \
  libayatana-appindicator3-dev \
  librsvg2-dev \
  libssl-dev \
  libasound2-dev \
  libspeechd-dev \
  pkg-config \
  build-essential

# Fedora
sudo dnf install \
  webkit2gtk4.1-devel \
  libayatana-appindicator-devel \
  openssl-devel \
  alsa-lib-devel \
  speech-dispatcher-devel
```

### Windows
- Visual Studio Build Tools 2019+ (select the "Desktop development with C++" workload)
- WebView2 Runtime (pre-installed on Windows 10 21H2+ and Windows 11)
- Rust MSVC toolchain: `rustup default stable-x86_64-pc-windows-msvc`

For full Windows build instructions and a PowerShell helper script, see **[docs/windows_build.md](windows_build.md)**.

---

## Repository Layout

```
VoxCtrl/
├── src/                    # Svelte frontend
│   ├── main.ts
│   ├── App.svelte
│   ├── stores/
│   │   ├── config.ts
│   │   └── status.ts
│   └── lib/
│       ├── Settings/
│       ├── Overlay/
│       └── History/
│
├── src-tauri/              # Tauri application shell
│   ├── Cargo.toml
│   ├── tauri.conf.json
│   └── src/
│       ├── main.rs
│       ├── lib.rs          # Main coordinator
│       ├── commands.rs     # IPC command handlers
│       └── state.rs        # AppState definition
│
├── crates/                 # Backend library crates
│   ├── voxctrl-config/
│   ├── voxctrl-audio/
│   ├── voxctrl-hotkeys/
│   ├── voxctrl-inference/
│   ├── voxctrl-routing/
│   ├── voxctrl-inject/
│   ├── voxctrl-tts/
│   ├── voxctrl-mcp/
│   ├── voxctrl-dbus/
│   ├── voxctrl-llm/
│   └── voxctrl-text/       # Shared text-processing (snippets, fuzzy vocab correction)
│
├── Cargo.toml              # Workspace definition
├── package.json            # Frontend deps
├── vite.config.ts
└── svelte.config.js
```

---

## Development Workflow

### Start Dev Server
```bash
npm install          # Install frontend deps (first time only)
npm run tauri dev    # Start Tauri + Vite in development mode
```

This:
1. Starts Vite dev server on `http://localhost:5173` with HMR
2. Compiles the Rust backend
3. Launches the app with the WebView pointed at Vite

Svelte changes hot-reload instantly. Rust changes trigger a backend recompile (typically 5–30s).

### Frontend Only
If you only need to work on the UI:
```bash
npm run dev
# Opens http://localhost:5173 in browser
# Note: Tauri commands won't work in browser — mock them if needed
```

### Type-Check the Frontend
```bash
npm run check        # Runs svelte-check (Svelte + TypeScript) against tsconfig.json
```
This is the same check CI runs on every push/PR. Tailwind `@apply`/`@reference`
warnings from `svelte-check` are expected (it does not parse Tailwind directives)
and do not fail the build; only genuine type errors do.

### Backend Only
```bash
cargo build -p voxctrl-inference  # Build a specific crate
cargo test -p voxctrl-config      # Test a specific crate
cargo check --workspace          # Type-check all crates
```

---

## Building for Production

### AppImage (Linux)
```bash
bash build_appimage.sh
# Output: VoxCtrl.AppImage in project root
```

The build script:
1. Runs `npm run tauri build` to produce a `.deb` bundle
2. Extracts the contents into an AppDir
3. Runs `appimagetool` to create the AppImage

### Standard Tauri Build
```bash
npm run tauri build
# Output: src-tauri/target/release/bundle/
#   Linux:   .deb, .AppImage
#   Windows: .msi, .exe (NSIS)
```

### CUDA GPU Acceleration (opt-in)

CUDA inference acceleration is disabled by default so the app builds on any machine. Enable it with the `cuda` cargo feature:

```bash
# Linux / macOS
npm run tauri build -- --features cuda

# Windows (PowerShell)
npm run tauri build -- --features cuda

Other opt-in features follow the same form. The Inflect-Micro-v2 TTS engine is
behind `inflect-micro` (it pulls in ONNX Runtime):

```bash
npm run tauri dev -- --features inflect-micro
npm run tauri build -- --features inflect-micro
```

The `--` is required — without it npm treats `--features` as its own flag and
fails with `EUNKNOWNCONFIG`. Note that the model **downloads** fine in a build
without the feature; only synthesis needs it, so Settings will show the engine
as ready while Test TTS stays disabled.
# Or use the helper script:
.\scripts\build_windows.ps1 -Cuda
```

The `cuda` feature propagates: `voxctrl-app/cuda` → `voxctrl-inference/cuda` → `whisper-rs/cuda`.

---

## Crate Development Guide

Each crate under `crates/` is self-contained. They are included in the workspace `Cargo.toml` and referenced by `src-tauri` as path dependencies.

### Adding a new crate
```bash
cargo new --lib crates/voxctrl-myfeature

# Add to Cargo.toml workspace members:
[workspace]
members = [
  ...
  "crates/voxctrl-myfeature",
]

# Reference from src-tauri/Cargo.toml:
voxctrl-myfeature = { path = "../crates/voxctrl-myfeature" }
```

### Crate Conventions
- Keep each crate focused on one domain
- Expose a minimal public API (`pub` on types/functions needed by callers)
- Use `tokio` for async where I/O is needed; keep CPU-heavy work on dedicated OS threads
- Pass channels rather than `Arc<Mutex<_>>` for data pipelines where possible

---

## Key Files to Understand

### `src-tauri/src/lib.rs`
The main coordinator. This is where the audio pipeline is assembled:
- Creates all channels
- Spawns the hotkey listener
- Spawns the audio recorder
- Spawns the inference worker
- Starts the MCP server
- Starts the DBus service
- Runs the Tauri event loop with the status ticker

When adding a new integration, this is typically where you wire it in.

### `src-tauri/src/commands.rs`
All `#[tauri::command]` handlers. Each command is a thin wrapper that reads/writes `AppState` or calls into a crate. Keep commands small — business logic belongs in crates.

### `crates/voxctrl-config/src/lib.rs`
The `AppConfig` struct is the source of truth for all settings. If you add a config option, add it here first, then expose it in the Settings UI.

### `crates/voxctrl-routing/src/models.rs`
Defines `OutputTarget`, `HotkeyBinding`, `DeliveryType`, `TargetProcessingConfig`, and `GestureType`. Add new delivery types or target fields here.

---

## Adding a New Output Target Type

1. Add a variant to `DeliveryType` enum in `crates/voxctrl-routing/src/models.rs`
2. Add any target-specific fields to `OutputTarget` in `crates/voxctrl-routing/src/models.rs`
3. Add a match arm in the router dispatch logic in `crates/voxctrl-routing/src/router.rs`
4. Update the TypeScript `OutputTarget` interface in `src/stores/config.ts`
5. Add the new type to the delivery type selector in `src/lib/Settings/RoutingTab.svelte`
6. Document in `docs/routing.md`

---

## Testing

VoxCtrl utilizes a multi-tiered, unified testing suite spanning Svelte frontend components, Rust backend crates, and end-to-end integration tests over local socket connections.

### Master Test Orchestrator

The easiest way to run the entire test suite (Rust, Svelte, and Pytest Integration) is via the master test runner script:

```bash
npm test
```

This runs `python3 scripts/run_tests.py`, which sequences the following three test suites and returns a consolidated exit code (cleanly skipping the integration tests with a warning if `pytest` is not installed on the system):
1. **Rust Backend tests** (`cargo test`)
2. **Svelte Frontend tests** (`npm run test:unit`)
3. **Python Integration tests** (`pytest tests/integration/`)

---

### Rust Backend Crate Tests

Backend logic, including settings schemas, migrations, routing models, and utilities, is tested using standard Rust/Cargo unit tests.

#### Running Backend Tests
```bash
# Run all tests across the entire workspace
cargo test --workspace

# Run tests for a specific backend crate
cargo test -p voxctrl-config
cargo test -p voxctrl-routing

# Run the native overlay engine's visualizer/animation tests
cargo test --bin voxctrl-overlay
```

The overlay engine tests (in `src-tauri/src/overlay.rs`) cover the load/unload animation spring (convergence, bounded overshoot, unload duration), the oscilloscope trace (orientation and clamping), the radar sweep/rings/blips geometry, the ocean wave paths (bottom edge locked to the stage, drain/fill mapping), and the VU LED matrix ballistics.

#### Writing Backend Tests
Backend unit tests are written inside their respective crate files within a `#[cfg(test)]` module block.
Example from `crates/voxctrl-config/src/lib.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_values() {
        let cfg = AppConfig::default();
        assert!(cfg.ui.auto_show_settings);
        assert_eq!(cfg.ui.overlay_style, OverlayStyle::BlueWave);
    }
}
```

---

### Svelte Frontend Unit & Component Tests

Frontend Svelte 5 components, settings views, and warning overlays are tested using **Vitest**, **JSDOM**, and **Svelte Testing Library**.

* **Test Location**: `tests/svelte/` (files ending in `.test.ts`, including component tests and utility script tests like `prepare-sidecar.test.ts`)
* **Framework Stack**: Vitest (runner), jsdom (DOM environment), `@testing-library/svelte` (rendering & selectors)

#### Running Frontend Tests
```bash
# Run all frontend tests once
npm run test:unit

# Run frontend tests in interactive watch mode
npx vitest
```

#### Mocking Tauri APIs
Tauri commands (`invoke`) and events (`listen`) are mocked inside Svelte tests using Vitest's `vi.mock` to ensure they run successfully in headless/JSDOM environments without a live Webview context.

Example from `tests/svelte/EngineTab.test.ts`:
```typescript
import { describe, test, expect, vi } from "vitest";
import { render, screen } from "@testing-library/svelte";
import EngineTab from "../../src/lib/Settings/EngineTab.svelte";

// Mock Tauri core commands
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (cmd, args) => {
    if (cmd === "check_model_downloaded") {
      return args.modelSize === "base"; // mock "base" downloaded, others missing
    }
    return true;
  }),
}));

// Mock Tauri events
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async () => {
    return () => {}; // return clean unsubscribe function
  }),
}));
```

#### Writing Svelte Tests
When testing Svelte components:
1. Render the component using `render(Component, { props })`.
2. Locate elements using Svelte Testing Library selectors (e.g., `screen.findByText` or `screen.queryByText`).
3. Assert behaviors using Vitest's `expect()`.

Example:
```typescript
describe("EngineTab.svelte Warning Banner", () => {
  test("shows warning banner if Whisper voice model is not downloaded", async () => {
    const mockConfig = {
      engine: {
        backend: "whisper-cpp",
        whisper_cpp: { model_size: "large-v3" },
      }
    } as any;

    render(EngineTab, { cfg: mockConfig });
    
    // Assert warning banner is found
    const title = await screen.findByText("Voice Model Not Downloaded");
    expect(title).not.toBeNull();
  });
});
```

---

### Python Socket Integration Tests

Integration tests verify end-to-end communication channels such as the Model Context Protocol (MCP) server over Unix domain sockets (`/tmp/voxctrl-mcp.sock`).

* **Test Location**: `tests/integration/` (files prefixed with `test_`)
* **Framework**: Pytest

#### Running Integration Tests
```bash
# Ensure pytest is installed
pip install pytest

# Run integration tests
pytest tests/integration/
```
*Note: These tests check for the live socket connection. If VoxCtrl is not currently running, these tests will gracefully skip to prevent false failure reports.*

#### Writing Integration Tests
Integration tests use the standard `pytest` framework, creating client socket connections to communicate with `/tmp/voxctrl-mcp.sock` over JSON-RPC.

Example:
```python
import socket
import json
import pytest
import os

SOCKET_PATH = "/tmp/voxctrl-mcp.sock"

@pytest.mark.skipif(not os.path.exists(SOCKET_PATH), reason="MCP Socket not running")
def test_mcp_handshake_and_tools():
    sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    sock.connect(SOCKET_PATH)
    try:
        # Send a standard JSON-RPC request to the MCP server
        payload = {"jsonrpc": "2.0", "id": 1, "method": "tools/list", "params": {}}
        sock.sendall((json.dumps(payload) + "\n").encode('utf-8'))
        
        # Read and parse response
        resp = json.loads(sock.recv(1024).decode('utf-8').strip())
        assert "result" in resp
        assert "tools" in resp["result"]
    finally:
        sock.close()
```

### Simulating and Testing udev Diagnostics

Linux global hotkeys require specific udev permissions and user group memberships configured by `install.sh`. To make testing these startup states safe and easy, developers can use the `VOXCTRL_TEST_UDEV_STATUS` environment variable to mock various diagnostic outcomes without mutating their own user accounts or system rules.

#### How Diagnostics Work (Linux)
The application employs robust permission checks at startup and via the `check_udev_status` Tauri IPC command:
1. **Rule File Compatibility:** The app checks for the existence of `/etc/udev/rules.d/99-voxctrl.rules`, `/etc/udev/rules.d/99-voxctl.rules` (legacy name), or `/etc/udev/rules.d/99-voxctr.rules` (legacy name). If any match, rules are recognized as configured.
2. **Active vs NSS Group Database Verification:** It checks if the active process session belongs to the `input` group. If missing, it queries the NSS system group database (`id -Gn <username>`) as a fallback. This handles persistent containerized development environments (where process group tokens do not refresh) gracefully, preventing false warning windows once the installer has been run.
3. **Windows Exclusions:** Non-Linux environments (such as Windows builds) completely compile out udev diagnostic checks on startup and return fully bypassed success payloads (`is_configured: true`), ensuring the warning screen never displays on Windows.

#### Why Test This?
* **Onboarding Verification**: Ensure that new users are clearly prompted to install required dependencies.
* **Troubleshooting Relogins**: Verify the specific advice prompting the user to reboot or log out if they ran `install.sh` but didn't refresh their session.
* **Layout Integrity**: Make sure the modal overlays perfectly on the dark obsidian theme on launch.

#### Mock Configurations

* **Simulate Missing Setup (Installer never run)**:
  Simulates `/etc/udev/rules.d/99-voxctrl.rules` does not exist:
  ```bash
  VOXCTRL_TEST_UDEV_STATUS=missing npm run tauri dev
  ```
  * **UI Outcome**: Spawns a standalone native window (`udev-warning`) in the foreground detailing the need for hardware udev rules, providing a direct **🔧 Setup System Integration** button to run setup automatically, and a **Continue Anyway** native window close pathway.

* **Simulate Relogin Required (Installer run but session not updated)**:
  Simulates that rules exist but the current shell process is missing `input` group permissions:
  ```bash
  VOXCTRL_TEST_UDEV_STATUS=relogin npm run tauri dev
  ```
  * **UI Outcome**: Spawns a standalone native window (`udev-warning`) displaying the explicit logout/relogin guidance (hiding the installer download CTA since the rules are already present).

* **Simulate Normal/Configured State (Bypasses checks)**:
  ```bash
  VOXCTRL_TEST_UDEV_STATUS=ok npm run tauri dev
  ```
  * **UI Outcome**: Spawns only the standard settings window; the diagnostic warning window remains completely hidden.


---

## Debugging

### Rust Logging
VoxCtrl uses the `log` crate with `env_logger`. Enable verbose output:
```bash
RUST_LOG=debug npm run tauri dev
RUST_LOG=voxctrl_inference=trace npm run tauri dev
```

### Frontend DevTools
In dev mode, right-click the Tauri window → Inspect Element to open WebKit DevTools.

### IPC Tracing
Add `console.log` around `invoke()` calls in Svelte, or add `println!` in command handlers in Rust.

### Audio Issues
```bash
# Check CPAL devices
RUST_LOG=cpal=debug npm run tauri dev

# Check PulseAudio
pactl list sources short
```

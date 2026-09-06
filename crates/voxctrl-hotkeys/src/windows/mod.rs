//! Windows hotkey backend: a Win32 low-level keyboard hook.
//!
//! Gesture recognition is the shared engine, so `hold`, `toggle`, `double_tap`
//! and `double_tap_hold` behave exactly as they do on Linux.
//!
//! # Shape
//!
//! Three pieces, and the split between them is the design:
//!
//! * The **hook procedure** runs inside whatever process generated the
//!   keystroke, and Windows silently unhooks a procedure that takes longer than
//!   `LowLevelHooksTimeout` — capped at one second since Windows 10 1709, with
//!   no notification to the application. So it does the least possible work:
//!   resolve a key id from a pure lookup, decide suppression from lock-free
//!   arrays and a `try_read`, push onto an unbounded channel, return.
//! * The **pump thread** owns the hook. `SetWindowsHookExW` requires the
//!   installing thread to run a message loop, and the hook is only ever called
//!   on that thread.
//! * The **worker thread** does everything expensive: matching, gesture timing,
//!   binding reloads, and sending gestures to the app.
//!
//! # Privacy
//!
//! This hook is called for every keystroke on the machine. That is the same
//! exposure as the evdev and X11 backends on Linux, and unlike the XDG portal,
//! so `Backend::WindowsHook` reports `sees_raw_keys() == true` and
//! `is_private() == false`. Nothing here logs, stores or forwards a key: each
//! event is matched against the user's bindings and dropped.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use tracing::{info, warn};
use voxctrl_routing::HotkeyBinding;

use crate::win_keys::{keymap, SuppressPlan};
use crate::{gestures::GestureEngine, keys::KeyMatcher, Backend, GestureSender, ListenerHealth};

use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, SetWindowsHookExW, TranslateMessage,
    UnhookWindowsHookEx, HC_ACTION, HHOOK, KBDLLHOOKSTRUCT, LLKHF_EXTENDED, MSG, WH_KEYBOARD_LL,
    WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

/// Marker on every keystroke VoxCtrl synthesises. Defined by `voxctrl-winput`,
/// which is the side that writes it; the hook is the side that reads it.
use voxctrl_winput::INJECTED_TAG;

/// How long the hook may go unheard before it is presumed dead and reinstalled.
///
/// Windows removes a hook that overruns `LowLevelHooksTimeout` without telling
/// anyone, so silence is the only symptom available. Long enough that a user who
/// simply is not typing does not churn the hook.
const WATCHDOG_SILENCE: Duration = Duration::from_secs(180);

/// How often the watchdog wakes to check that silence.
const WATCHDOG_TICK: Duration = Duration::from_secs(30);

/// One key transition as the hook saw it.
struct RawKey {
    id: usize,
    down: bool,
}

// ── State the hook procedure reaches without capturing ────────────────────────
//
// A `HOOKPROC` is a bare `extern "system" fn`, so everything it touches is
// static. Each of these is either atomic or read with `try_read`; the hook never
// blocks on a lock.

static EVENTS: Mutex<Option<crossbeam_channel::Sender<RawKey>>> = Mutex::new(None);
static PLAN: RwLock<Option<SuppressPlan>> = RwLock::new(None);
static PRESSED: [AtomicBool; keymap::NAMES.len()] =
    [const { AtomicBool::new(false) }; keymap::NAMES.len()];
static SWALLOWED: [AtomicBool; keymap::NAMES.len()] =
    [const { AtomicBool::new(false) }; keymap::NAMES.len()];
/// Monotonic count of events the hook has handled, read by the watchdog.
static SEEN: AtomicUsize = AtomicUsize::new(0);
/// Thread id of the pump, so the watchdog can ask it to exit.
static PUMP_THREAD: AtomicU32 = AtomicU32::new(0);

/// Sender cached outside the mutex for the hook's fast path.
///
/// `crossbeam_channel::Sender` is `Sync` and its `send` on an unbounded channel
/// does not block, so the hook can use it directly once it exists.
static SENDER: RwLock<Option<crossbeam_channel::Sender<RawKey>>> = RwLock::new(None);

unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // Anything but HC_ACTION must be passed straight on, untouched.
    if code != HC_ACTION as i32 {
        return unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) };
    }

    let info = unsafe { &*(lparam as *const KBDLLHOOKSTRUCT) };

    // VoxCtrl's own synthesised keystrokes. Letting these through would mean a
    // dictation containing the binding's keys re-triggered the binding.
    if info.dwExtraInfo == INJECTED_TAG {
        return unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) };
    }

    let msg = wparam as u32;
    let down = matches!(msg, WM_KEYDOWN | WM_SYSKEYDOWN);
    let up = matches!(msg, WM_KEYUP | WM_SYSKEYUP);
    if !down && !up {
        return unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) };
    }

    let extended = info.flags & LLKHF_EXTENDED != 0;
    let Some(id) = keymap::lookup(info.scanCode, extended, info.vkCode) else {
        return unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) };
    };

    SEEN.fetch_add(1, Ordering::Relaxed);

    // Auto-repeat: Windows re-sends key-down while a key is held. The gesture
    // engine measures hold duration from the first press, so a repeat must not
    // look like a fresh one.
    let repeat = down && PRESSED[id].load(Ordering::Relaxed);

    let swallow = if down {
        if repeat {
            // Keep swallowing a key whose initial press was swallowed, or the
            // repeats leak into the focused window.
            SWALLOWED[id].load(Ordering::Relaxed)
        } else {
            // `try_read` rather than `read`: a reload holding the writer for a
            // moment must never stall the hook into Windows' timeout. Failing
            // to suppress for one keystroke is a far smaller fault than being
            // silently unhooked.
            let decided = match PLAN.try_read() {
                Ok(plan) => plan
                    .as_ref()
                    .map(|plan| plan.should_swallow(id, &PRESSED))
                    .unwrap_or(false),
                Err(_) => false,
            };
            SWALLOWED[id].store(decided, Ordering::Relaxed);
            decided
        }
    } else {
        // Release: mirror whatever happened to the press, so an application
        // never sees a key-up it had no key-down for.
        SWALLOWED[id].swap(false, Ordering::Relaxed)
    };

    if down {
        PRESSED[id].store(true, Ordering::Relaxed);
    } else {
        PRESSED[id].store(false, Ordering::Relaxed);
    }

    if !repeat {
        if let Ok(guard) = SENDER.try_read() {
            if let Some(tx) = guard.as_ref() {
                let _ = tx.send(RawKey { id, down });
            }
        }
    }

    if swallow {
        // Non-zero stops the event reaching any further hook or application.
        return 1;
    }
    unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
}

// ── Public entry point ────────────────────────────────────────────────────────

pub fn start(
    bindings: Vec<HotkeyBinding>,
    tx: GestureSender,
    rx_reload: crate::ReloaderReceiver,
    health: Arc<ListenerHealth>,
) {
    // The hook needs no device permissions and no desktop cooperation, so there
    // is nothing to scan and nothing that can be denied.
    health.set_supported(true);
    health.record_scan(1, 0);
    health.set_keyboards_open(1);
    health.set_backend(Backend::WindowsHook);

    let failed = health.clone();
    if let Err(e) = std::thread::Builder::new()
        .name("voxctrl-winhook".into())
        .spawn(move || run(bindings, tx, rx_reload, health))
    {
        failed.set_backend_failed(format!("cannot start the keyboard hook: {e}"));
    }
}

fn run(
    bindings: Vec<HotkeyBinding>,
    tx: GestureSender,
    rx_reload: crate::ReloaderReceiver,
    health: Arc<ListenerHealth>,
) {
    let (raw_tx, raw_rx) = crossbeam_channel::unbounded::<RawKey>();
    *PLAN.write().unwrap() = Some(SuppressPlan::build(&bindings));
    *SENDER.write().unwrap() = Some(raw_tx.clone());
    *EVENTS.lock().unwrap() = Some(raw_tx);

    // Worker: everything the hook is too time-constrained to do itself.
    let worker_health = health.clone();
    let worker = std::thread::Builder::new()
        .name("voxctrl-winhook-match".into())
        .spawn(move || match_loop(bindings, tx, rx_reload, raw_rx, worker_health));
    if let Err(e) = worker {
        health.set_backend_failed(format!("cannot start the hotkey matcher: {e}"));
        return;
    }

    spawn_watchdog(health.clone());

    // Pump: owns the hook and must keep a message loop running for it.
    loop {
        match pump_once() {
            PumpOutcome::Reinstall => {
                warn!("keyboard hook stopped; reinstalling");
                release_all(&raw_tx_from_static());
            }
            PumpOutcome::CannotHook(e) => {
                health.set_backend_failed(format!("cannot install the keyboard hook: {e}"));
                return;
            }
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

fn raw_tx_from_static() -> Option<crossbeam_channel::Sender<RawKey>> {
    EVENTS.lock().ok().and_then(|g| g.clone())
}

/// Tell the matcher that everything held is now up.
///
/// The hook going away means no release will ever arrive for a key the user is
/// still holding, which would otherwise leave a `hold` gesture recording
/// forever.
fn release_all(tx: &Option<crossbeam_channel::Sender<RawKey>>) {
    let Some(tx) = tx else { return };
    for (id, held) in PRESSED.iter().enumerate() {
        if held.swap(false, Ordering::Relaxed) {
            let _ = tx.send(RawKey { id, down: false });
        }
        SWALLOWED[id].store(false, Ordering::Relaxed);
    }
}

enum PumpOutcome {
    /// The hook is gone and should be installed again.
    Reinstall,
    /// The hook could not be installed at all.
    CannotHook(std::io::Error),
}

fn pump_once() -> PumpOutcome {
    let hook: HHOOK = unsafe {
        SetWindowsHookExW(
            WH_KEYBOARD_LL,
            Some(hook_proc),
            std::ptr::null_mut(),
            0,
        )
    };
    if hook.is_null() {
        return PumpOutcome::CannotHook(std::io::Error::last_os_error());
    }
    info!("Win32 low-level keyboard hook installed");

    PUMP_THREAD.store(
        unsafe { windows_sys::Win32::System::Threading::GetCurrentThreadId() },
        Ordering::SeqCst,
    );

    // `GetMessageW` returns 0 on WM_QUIT and -1 on error; either ends the loop
    // and the hook is reinstalled. The messages themselves are of no interest —
    // the loop exists purely because a low-level hook is only dispatched to a
    // thread that is pumping.
    let mut msg: MSG = unsafe { std::mem::zeroed() };
    loop {
        let got = unsafe { GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) };
        if got <= 0 {
            break;
        }
        unsafe {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }

    unsafe {
        let _ = UnhookWindowsHookEx(hook);
    }
    PumpOutcome::Reinstall
}

/// Watch for the silent unhook.
///
/// Windows removes a hook procedure that overruns its timeout and tells the
/// application nothing, so the only evidence is that events stop arriving. If
/// the machine has been quiet for longer than a person plausibly pauses, ask the
/// pump to drop out of its message loop and install a fresh hook.
fn spawn_watchdog(health: Arc<ListenerHealth>) {
    let _ = std::thread::Builder::new()
        .name("voxctrl-winhook-watchdog".into())
        .spawn(move || {
            let mut last_seen = SEEN.load(Ordering::Relaxed);
            let mut quiet_since = Instant::now();
            loop {
                std::thread::sleep(WATCHDOG_TICK);
                let seen = SEEN.load(Ordering::Relaxed);
                if seen != last_seen {
                    last_seen = seen;
                    quiet_since = Instant::now();
                    continue;
                }
                if quiet_since.elapsed() < WATCHDOG_SILENCE {
                    continue;
                }
                let thread = PUMP_THREAD.load(Ordering::SeqCst);
                if thread == 0 {
                    continue;
                }
                warn!(
                    "no keyboard events for {}s; reinstalling the hook in case Windows \
                     removed it for overrunning LowLevelHooksTimeout",
                    WATCHDOG_SILENCE.as_secs()
                );
                health.set_backend(Backend::WindowsHook);
                unsafe {
                    let _ = windows_sys::Win32::UI::WindowsAndMessaging::PostThreadMessageW(
                        thread,
                        windows_sys::Win32::UI::WindowsAndMessaging::WM_QUIT,
                        0,
                        0,
                    );
                }
                quiet_since = Instant::now();
            }
        });
}

/// Match raw keys against bindings and drive the gesture engine.
fn match_loop(
    bindings: Vec<HotkeyBinding>,
    tx: GestureSender,
    rx_reload: crate::ReloaderReceiver,
    raw_rx: crossbeam_channel::Receiver<RawKey>,
    _health: Arc<ListenerHealth>,
) {
    let mut engine = GestureEngine::new(bindings.clone());
    let mut matcher = KeyMatcher::new(bindings);

    loop {
        crossbeam_channel::select! {
            recv(rx_reload) -> msg => {
                let Ok(new_bindings) = msg else { break };
                info!("windows hotkey loop: reloading {} bindings", new_bindings.len());
                // Reset before swapping: a gesture recorded against the old
                // bindings has no owner afterwards.
                engine.reset(&tx);
                for (id, transition) in matcher.clear() {
                    engine.apply(&id, transition, &tx);
                }
                engine.reload(new_bindings.clone());
                matcher.reload(new_bindings.clone());
                *PLAN.write().unwrap() = Some(SuppressPlan::build(&new_bindings));
            }
            recv(raw_rx) -> msg => {
                let Ok(raw) = msg else { break };
                let name = keymap::name(raw.id);
                for (id, transition) in matcher.on_key(name, raw.down) {
                    engine.apply(&id, transition, &tx);
                }
            }
        }
    }

    engine.reset(&tx);
}

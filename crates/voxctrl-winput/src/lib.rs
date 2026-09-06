//! Synthesised keyboard input on Windows, and the marker that identifies it.
//!
//! Everything that puts text into the focused window on Windows goes through
//! here, so there is one implementation to get right and one marker for the
//! hotkey hook to recognise.
//!
//! This replaced a `powershell.exe -Command SendKeys::SendWait` call, which was
//! wrong in a way that only showed up on real dictation. `SendKeys` reads
//! `+ ^ % ~ ( ) { } [ ]` as its own syntax, so "50% (a+b)" arrived as "50"
//! followed by a couple of stray chords, and "array[0]" as "array0". The old
//! code base64-encoded the payload — which does protect it from *PowerShell*
//! parsing, and was a real defence — but the decoded string still went through
//! SendKeys' own escaping, so ordinary prose came out mangled. It also spawned
//! a process per dictation, on the path where latency is most visible.
//!
//! `SendInput` with `KEYEVENTF_UNICODE` carries the character itself rather
//! than a keystroke to be interpreted: no escaping exists to get wrong, no
//! keyboard layout is consulted, and nothing is spawned.
//!
//! The chunking logic below is deliberately outside the platform gate so its
//! tests run on the Linux lane, where the suite actually runs.

/// Marker stamped into the `dwExtraInfo` of every keystroke VoxCtrl
/// synthesises, so its own keyboard hook can tell the app's output from the
/// user typing.
///
/// Without it, dictating text that completes a binding would re-trigger that
/// binding from VoxCtrl's own output.
///
/// Arbitrary, but deliberately not a small integer: other software stamps
/// `dwExtraInfo` too, and 0 or 1 would collide.
pub const INJECTED_TAG: usize = 0x9605_C731;

/// Above this many characters, typing is abandoned for a clipboard paste.
///
/// `SendInput` is a single call whatever the length, but the receiving
/// application still processes one message per character, and a long
/// transcription visibly crawls into editors that do syntax work per keystroke.
/// A paste is one operation regardless of size.
pub const PASTE_THRESHOLD_CHARS: usize = 2000;

/// How many UTF-16 code units to submit per `SendInput` call.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
const CHUNK_UNITS: usize = 512;

/// Split a UTF-16 buffer into chunks of at most `max` units without ever
/// separating a surrogate pair.
///
/// A high surrogate delivered in one `SendInput` call and its low surrogate in
/// the next is not a character: the receiving application sees two lone
/// surrogates and renders replacement glyphs. Emoji and the rarer CJK blocks
/// are exactly the text this protects.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn chunk_boundaries(units: &[u16], max: usize) -> Vec<std::ops::Range<usize>> {
    let is_high_surrogate = |u: u16| (0xD800..0xDC00).contains(&u);

    let mut ranges = Vec::new();
    let mut start = 0;
    while start < units.len() {
        let mut end = (start + max).min(units.len());
        // A high surrogate is never the last unit of a chunk.
        if end < units.len() && is_high_surrogate(units[end - 1]) {
            if end - start > 1 {
                // Push the pair into the next chunk.
                end -= 1;
            } else {
                // The chunk is the high surrogate alone, so there is nothing to
                // push it into — dropping it would leave an empty range and
                // make no progress. Take the low surrogate too and run one unit
                // over `max`, which the API does not mind.
                end += 1;
            }
        }
        ranges.push(start..end);
        start = end;
    }
    ranges
}

/// Whether `text` is long enough to be worth pasting rather than typing.
pub fn prefers_paste(text: &str) -> bool {
    text.chars().count() > PASTE_THRESHOLD_CHARS
}

#[cfg(target_os = "windows")]
mod imp {
    use anyhow::{bail, Result};
    use tracing::debug;

    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP,
        KEYEVENTF_UNICODE, VK_CONTROL, VK_V,
    };

    use super::{chunk_boundaries, INJECTED_TAG, CHUNK_UNITS};

    /// Type `text` into whatever currently has focus, character by character.
    pub fn type_text(text: &str) -> Result<()> {
        if text.is_empty() {
            return Ok(());
        }
        let units: Vec<u16> = text.encode_utf16().collect();
        for range in chunk_boundaries(&units, CHUNK_UNITS) {
            send_unicode(&units[range])?;
        }
        debug!(chars = text.chars().count(), "typed via SendInput");
        Ok(())
    }

    fn send_unicode(units: &[u16]) -> Result<()> {
        let mut inputs: Vec<INPUT> = Vec::with_capacity(units.len() * 2);
        for unit in units {
            inputs.push(key_event(0, *unit, KEYEVENTF_UNICODE));
            inputs.push(key_event(0, *unit, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP));
        }
        submit(&inputs)
    }

    fn key_event(vk: u16, scan: u16, flags: u32) -> INPUT {
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: vk,
                    wScan: scan,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: INJECTED_TAG,
                },
            },
        }
    }

    fn submit(inputs: &[INPUT]) -> Result<()> {
        if inputs.is_empty() {
            return Ok(());
        }
        let sent = unsafe {
            SendInput(
                inputs.len() as u32,
                inputs.as_ptr(),
                std::mem::size_of::<INPUT>() as i32,
            )
        };
        if sent as usize != inputs.len() {
            // The usual cause is UIPI: a more-privileged window has focus, and
            // an unelevated process may not synthesise input into it. Say so,
            // because the alternative symptom is dictation that silently does
            // nothing.
            bail!(
                "SendInput delivered {sent} of {} events ({}). If the focused window \
                 belongs to an elevated application, Windows blocks input from \
                 unelevated processes into it.",
                inputs.len(),
                std::io::Error::last_os_error()
            );
        }
        Ok(())
    }

    /// Put `text` on the clipboard and press Ctrl+V.
    ///
    /// The previous clipboard contents are restored afterwards: taking the
    /// user's clipboard permanently as a side effect of dictating is its own
    /// bug.
    pub fn paste_text(text: &str) -> Result<()> {
        let previous = {
            let mut cb = arboard::Clipboard::new()?;
            let previous = cb.get_text().ok();
            cb.set_text(text)?;
            previous
        };

        let result = press_ctrl_v();

        // Give the target a moment to read the clipboard before it is handed
        // back. Restoring immediately races the paste, and losing the
        // transcription is worse than briefly holding the clipboard.
        std::thread::sleep(std::time::Duration::from_millis(120));
        if let Some(previous) = previous {
            if let Ok(mut cb) = arboard::Clipboard::new() {
                let _ = cb.set_text(previous);
            }
        }

        result
    }

    fn press_ctrl_v() -> Result<()> {
        let inputs = [
            key_event(VK_CONTROL, 0, 0),
            key_event(VK_V, 0, 0),
            key_event(VK_V, 0, KEYEVENTF_KEYUP),
            key_event(VK_CONTROL, 0, KEYEVENTF_KEYUP),
        ];
        submit(&inputs)
    }

    /// Deliver `text` by whichever route suits its length.
    pub fn deliver(text: &str) -> Result<()> {
        if super::prefers_paste(text) {
            paste_text(text)
        } else {
            type_text(text)
        }
    }
}

#[cfg(target_os = "windows")]
pub use imp::{deliver, paste_text, type_text};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_surrogate_pair_is_never_split_across_chunks() {
        // "😀" is one code point and two UTF-16 units. Delivering them in
        // separate SendInput calls makes the target render two replacement
        // glyphs instead of the emoji.
        let units: Vec<u16> = "aa😀bb".encode_utf16().collect();
        for max in 1..=units.len() + 2 {
            for range in chunk_boundaries(&units, max) {
                if let Some(last) = units[range.clone()].last() {
                    assert!(
                        !(0xD800..0xDC00).contains(last),
                        "max={max}: chunk {range:?} ends on a high surrogate"
                    );
                }
            }
        }
    }

    #[test]
    fn chunking_covers_the_whole_string_exactly_once() {
        let units: Vec<u16> = "the quick brown 🦊 jumps".encode_utf16().collect();
        for max in 1..=units.len() + 2 {
            let rejoined: Vec<u16> = chunk_boundaries(&units, max)
                .iter()
                .flat_map(|r| units[r.clone()].to_vec())
                .collect();
            assert_eq!(rejoined, units, "max={max} lost or duplicated text");
        }
    }

    #[test]
    fn chunking_always_makes_progress() {
        // Regression guard. Shrinking a chunk to keep a surrogate pair together
        // used to be able to shrink it to nothing when the chunk was one unit
        // long, so the loop pushed empty ranges forever and the process was
        // killed for exhausting memory.
        let units: Vec<u16> = "😀😀😀".encode_utf16().collect();
        for max in 1..=4 {
            let ranges = chunk_boundaries(&units, max);
            assert!(
                ranges.iter().all(|r| !r.is_empty()),
                "max={max} produced an empty chunk"
            );
            assert!(ranges.len() <= units.len(), "max={max} did not terminate cleanly");
        }
    }

    #[test]
    fn an_empty_string_produces_no_chunks() {
        assert!(chunk_boundaries(&[], CHUNK_UNITS).is_empty());
    }

    #[test]
    fn the_metacharacters_sendkeys_ate_are_just_text_here() {
        // The regression this crate exists for. These are all SendKeys syntax —
        // `%` alt, `^` ctrl, `+` shift, `~` enter, and the four bracket pairs
        // are grouping — so the old PowerShell path delivered chords and
        // dropped characters. As UTF-16 code units they are unremarkable, and
        // every one survives chunking.
        for sample in ["50% of users", "f(x) = a + b", "array[0]", "{braces}", "a~b^c"] {
            let units: Vec<u16> = sample.encode_utf16().collect();
            let rejoined: Vec<u16> = chunk_boundaries(&units, 4)
                .iter()
                .flat_map(|r| units[r.clone()].to_vec())
                .collect();
            assert_eq!(
                String::from_utf16(&rejoined).unwrap(),
                sample,
                "{sample:?} did not survive"
            );
        }
    }

    #[test]
    fn long_text_prefers_a_paste_and_short_text_does_not() {
        assert!(prefers_paste(&"a".repeat(PASTE_THRESHOLD_CHARS + 1)));
        assert!(!prefers_paste(&"a".repeat(PASTE_THRESHOLD_CHARS)));
        assert!(!prefers_paste("hello"));
    }
}

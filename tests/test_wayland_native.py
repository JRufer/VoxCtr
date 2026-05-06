"""
Tests for the Wayland-native implementation.

Covers:
  - PortalInjector keysym resolution (ASCII, control chars, full Unicode)
  - OverlaySubprocessProxy interface and lifecycle helpers
  - text_injector.py evdev constants / fallback numeric codes
"""
import os
import sys
import unittest
from unittest.mock import MagicMock, patch, call
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent.parent / "src"))


# ── Helpers ───────────────────────────────────────────────────────────────────

def _make_portal_injector():
    """Return a PortalInjector without calling __init__ (avoids dbus import)."""
    # portal_injector imports dbus at module level; skip that by constructing
    # the object directly and only testing pure-Python methods.
    import importlib, types

    # Provide a minimal dbus stub so the module loads in environments without dbus.
    if "dbus" not in sys.modules:
        stub = types.ModuleType("dbus")
        stub.SessionBus = MagicMock
        stub.Interface  = MagicMock
        stub.UInt32     = int
        sys.modules["dbus"] = stub

    from portal_injector import PortalInjector
    obj = PortalInjector.__new__(PortalInjector)
    return obj


# ── PortalInjector keysym tests ───────────────────────────────────────────────

class TestPortalInjectorKeysym(unittest.TestCase):

    def setUp(self):
        self.pi = _make_portal_injector()

    # ASCII range: keysym == codepoint
    def test_ascii_printable(self):
        for ch in "ABCabc !@#$%^&*()_+-=":
            ks = self.pi._char_to_keysym(ch)
            self.assertEqual(ks, ord(ch), f"Expected {ord(ch):#x} for {ch!r}, got {ks!r}")

    def test_ascii_first_printable(self):
        self.assertEqual(self.pi._char_to_keysym(' '), 0x20)

    def test_ascii_last_printable(self):
        self.assertEqual(self.pi._char_to_keysym('~'), 0x7E)

    # Control characters
    def test_newline_maps_to_return_keysym(self):
        self.assertEqual(self.pi._char_to_keysym('\n'), 0xFF0D)

    def test_tab_maps_to_tab_keysym(self):
        self.assertEqual(self.pi._char_to_keysym('\t'), 0xFF09)

    def test_null_returns_none(self):
        self.assertIsNone(self.pi._char_to_keysym('\x00'))

    def test_bell_returns_none(self):
        self.assertIsNone(self.pi._char_to_keysym('\x07'))

    # Latin Extended / accented characters (XKB Unicode keysym: 0x01000000 + cp)
    def test_e_acute(self):
        # U+00E9 → keysym 0x010000E9
        self.assertEqual(self.pi._char_to_keysym('é'), 0x010000E9)

    def test_u_umlaut(self):
        # U+00FC → keysym 0x010000FC
        self.assertEqual(self.pi._char_to_keysym('ü'), 0x010000FC)

    def test_n_tilde(self):
        # U+00F1 → keysym 0x010000F1
        self.assertEqual(self.pi._char_to_keysym('ñ'), 0x010000F1)

    def test_euro_sign(self):
        # U+20AC → keysym 0x010020AC
        self.assertEqual(self.pi._char_to_keysym('€'), 0x010020AC)

    # CJK
    def test_cjk_unified_ideograph(self):
        # U+4E2D (中) → keysym 0x01004E2D
        self.assertEqual(self.pi._char_to_keysym('中'), 0x01004E2D)

    # Emoji / high Unicode
    def test_emoji(self):
        # U+1F600 (😀) → keysym 0x0101F600
        self.assertEqual(self.pi._char_to_keysym('😀'), 0x0101F600)

    def test_formula_correctness(self):
        # Verify the formula 0x01000000 + codepoint is applied for all non-ASCII > 0x7E
        for ch in "àáâãäåæçèéêëìíîïðñòóôõöøùúûüýþÿ":
            ks = self.pi._char_to_keysym(ch)
            expected = 0x01000000 + ord(ch)
            self.assertEqual(ks, expected, f"Formula mismatch for {ch!r}")


# ── OverlaySubprocessProxy tests ─────────────────────────────────────────────

class TestOverlaySubprocessProxy(unittest.TestCase):

    def _make_proxy(self):
        from gui.overlay_manager import OverlaySubprocessProxy
        return OverlaySubprocessProxy("voice_card")

    def test_initial_state_has_no_subprocess(self):
        proxy = self._make_proxy()
        self.assertIsNone(proxy._proc)
        self.assertIsNone(proxy._conn)
        self.assertIsNone(proxy._shm)

    def test_cleanup_is_safe_when_nothing_started(self):
        proxy = self._make_proxy()
        proxy._cleanup()   # should not raise
        self.assertIsNone(proxy._shm)
        self.assertIsNone(proxy._conn)

    def test_stop_is_safe_before_start(self):
        proxy = self._make_proxy()
        proxy.stop()   # should not raise

    @unittest.skipUnless(__import__("importlib").util.find_spec("numpy"), "numpy not installed")
    def test_update_audio_is_safe_before_start(self):
        import numpy as np
        proxy = self._make_proxy()
        proxy.update_audio(np.zeros(1024, dtype=np.float32))   # no subprocess yet, should no-op

    def test_interface_has_required_methods(self):
        from gui.overlay_manager import OverlaySubprocessProxy
        for method in ("show_mode", "hide_mode", "update_audio", "swap", "show_tts", "hide_tts", "stop"):
            self.assertTrue(
                callable(getattr(OverlaySubprocessProxy, method, None)),
                f"OverlaySubprocessProxy.{method} is missing or not callable",
            )

    def test_overlay_id_stored_on_init(self):
        from gui.overlay_manager import OverlaySubprocessProxy
        proxy = OverlaySubprocessProxy("pulse")
        self.assertEqual(proxy._overlay_id, "pulse")

    def test_swap_updates_overlay_id(self):
        proxy = self._make_proxy()
        with patch.object(proxy, "_send"):
            proxy.swap("waveform")
        self.assertEqual(proxy._overlay_id, "waveform")

    def test_swap_sends_command(self):
        proxy = self._make_proxy()
        with patch.object(proxy, "_send") as mock_send:
            proxy.swap("pulse")
        mock_send.assert_called_once_with(("swap", "pulse"))

    def test_show_mode_sends_command(self):
        proxy = self._make_proxy()
        with patch.object(proxy, "_send") as mock_send:
            proxy.show_mode("Hermes Agent")
        mock_send.assert_called_once_with(("show", "Hermes Agent"))

    def test_hide_mode_sends_command(self):
        proxy = self._make_proxy()
        with patch.object(proxy, "_send") as mock_send:
            proxy.hide_mode()
        mock_send.assert_called_once_with(("hide",))

    def test_show_tts_sends_command(self):
        proxy = self._make_proxy()
        with patch.object(proxy, "_send") as mock_send:
            proxy.show_tts("Hello world", "Agent")
        mock_send.assert_called_once_with(("tts_show", "Hello world", "Agent"))

    def test_hide_tts_sends_command(self):
        proxy = self._make_proxy()
        with patch.object(proxy, "_send") as mock_send:
            proxy.hide_tts()
        mock_send.assert_called_once_with(("tts_hide",))

    @unittest.skipUnless(__import__("importlib").util.find_spec("numpy"), "numpy not installed")
    def test_update_audio_writes_to_shm_and_sends_notify(self):
        import numpy as np

        proxy = self._make_proxy()
        # Simulate an active shared memory buffer
        fake_buf = np.zeros(1024, dtype=np.float32)
        proxy._shm_buf = fake_buf
        proxy._seq = 0

        data = np.ones(1024, dtype=np.float32) * 0.5
        with patch.object(proxy, "_send") as mock_send:
            proxy.update_audio(data)

        # Buffer should have been written
        self.assertAlmostEqual(float(fake_buf[0]), 0.5, places=5)
        # Sequence counter incremented and notification sent
        self.assertEqual(proxy._seq, 1)
        mock_send.assert_called_once_with(("audio", 1))


# ── text_injector evdev constants ─────────────────────────────────────────────

class TestTextInjectorEvdevConstants(unittest.TestCase):

    def test_no_magic_keycode_strings_in_source(self):
        src = (Path(__file__).parent.parent / "src" / "text_injector.py").read_text()
        self.assertNotIn("'29:0'",  src, "Magic keycode '29:0' (KEY_LEFTCTRL) still in text_injector.py")
        self.assertNotIn("'125:0'", src, "Magic keycode '125:0' (KEY_LEFTMETA) still in text_injector.py")
        self.assertNotIn("'56:0'",  src, "Magic keycode '56:0' (KEY_LEFTALT) still in text_injector.py")

    def test_ecodes_import_present(self):
        src = (Path(__file__).parent.parent / "src" / "text_injector.py").read_text()
        self.assertIn("from evdev import ecodes", src)

    def test_fallback_integers_are_correct(self):
        src = (Path(__file__).parent.parent / "src" / "text_injector.py").read_text()
        # The numeric fallback list [29, 125, 56] must still be present as a safeguard.
        self.assertIn("29, 125, 56", src, "Fallback integer list missing from text_injector.py")


# ── overlay_subprocess.py structure ──────────────────────────────────────────

class TestOverlaySubprocessFile(unittest.TestCase):

    def _src(self):
        return (Path(__file__).parent.parent / "src" / "gui" / "overlay_subprocess.py").read_text()

    def test_file_exists(self):
        p = Path(__file__).parent.parent / "src" / "gui" / "overlay_subprocess.py"
        self.assertTrue(p.exists(), "overlay_subprocess.py not found")

    def test_layer_shell_env_set_before_qapplication(self):
        src = self._src()
        layer_shell_pos   = src.index("QT_WAYLAND_SHELL_INTEGRATION")
        qapplication_pos  = src.index("QApplication")
        self.assertLess(
            layer_shell_pos, qapplication_pos,
            "QT_WAYLAND_SHELL_INTEGRATION must be set before QApplication is imported/created",
        )

    def test_layer_shell_value_is_layer_shell(self):
        self.assertIn('"layer-shell"', self._src())

    def test_handles_quit_command(self):
        self.assertIn('"quit"', self._src())

    def test_handles_tts_show_and_hide_commands(self):
        src = self._src()
        self.assertIn('"tts_show"', src)
        self.assertIn('"tts_hide"', src)

    def test_uses_shared_memory_for_audio(self):
        self.assertIn("SharedMemory", self._src())


# ── Flatpak manifest ──────────────────────────────────────────────────────────

class TestFlatpakManifest(unittest.TestCase):

    def _manifest(self):
        p = Path(__file__).parent.parent / "ai.whisperwayland.Dictation.yaml"
        return p.read_text()

    def test_fallback_x11_not_present(self):
        self.assertNotIn(
            "fallback-x11",
            self._manifest(),
            "--socket=fallback-x11 must be removed from the Flatpak manifest",
        )

    def test_wayland_socket_present(self):
        self.assertIn("--socket=wayland", self._manifest())


if __name__ == "__main__":
    unittest.main()

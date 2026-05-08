"""Tests for the key-capture helper functions used by _KeyCaptureWidget."""
import sys
import os
import pytest

sys.path.insert(0, os.path.join(os.path.dirname(__file__), '..', 'src'))

from gui.keybind_utils import fmt_evdev_keys, sorted_evdev_keys, NICE_KEY_NAMES


class TestFmtEvdevKeys:
    def test_empty(self):
        assert fmt_evdev_keys([]) == ""

    def test_single_nice_key(self):
        assert fmt_evdev_keys(["KEY_SPACE"]) == "Space"

    def test_modifier_plus_key(self):
        result = fmt_evdev_keys(["KEY_LEFTCTRL", "KEY_SPACE"])
        assert "Ctrl (L)" in result
        assert "Space" in result
        assert "  +  " in result

    def test_unknown_key_strips_prefix(self):
        # Keys not in the nice-name map get KEY_ stripped and title-cased
        result = fmt_evdev_keys(["KEY_F5"])
        assert result == "F5"

    def test_all_known_modifiers_have_nice_names(self):
        for evdev_name in NICE_KEY_NAMES:
            assert fmt_evdev_keys([evdev_name]) == NICE_KEY_NAMES[evdev_name]

    def test_join_separator(self):
        result = fmt_evdev_keys(["KEY_LEFTCTRL", "KEY_LEFTALT", "KEY_DELETE"])
        parts = result.split("  +  ")
        assert len(parts) == 3

    def test_unknown_key_title_case(self):
        assert fmt_evdev_keys(["KEY_VOLUMEUP"]) == "Volumeup"


class TestSortedEvdevKeys:
    def test_modifier_before_trigger(self):
        keys = ["KEY_SPACE", "KEY_LEFTCTRL"]
        result = sorted_evdev_keys(keys)
        assert result.index("KEY_LEFTCTRL") < result.index("KEY_SPACE")

    def test_multiple_modifiers_before_trigger(self):
        keys = ["KEY_A", "KEY_LEFTSHIFT", "KEY_LEFTCTRL"]
        result = sorted_evdev_keys(keys)
        assert result.index("KEY_LEFTCTRL") < result.index("KEY_A")
        assert result.index("KEY_LEFTSHIFT") < result.index("KEY_A")

    def test_meta_before_ctrl(self):
        keys = ["KEY_LEFTCTRL", "KEY_LEFTMETA"]
        result = sorted_evdev_keys(keys)
        assert result.index("KEY_LEFTMETA") < result.index("KEY_LEFTCTRL")

    def test_ctrl_before_alt(self):
        keys = ["KEY_LEFTALT", "KEY_LEFTCTRL"]
        result = sorted_evdev_keys(keys)
        assert result.index("KEY_LEFTCTRL") < result.index("KEY_LEFTALT")

    def test_alt_before_shift(self):
        keys = ["KEY_LEFTSHIFT", "KEY_LEFTALT"]
        result = sorted_evdev_keys(keys)
        assert result.index("KEY_LEFTALT") < result.index("KEY_LEFTSHIFT")

    def test_only_modifiers_preserved(self):
        keys = ["KEY_LEFTSHIFT", "KEY_LEFTCTRL"]
        result = sorted_evdev_keys(keys)
        assert len(result) == 2
        assert set(result) == {"KEY_LEFTSHIFT", "KEY_LEFTCTRL"}

    def test_no_modifiers_unchanged_set(self):
        keys = ["KEY_A", "KEY_B"]
        result = sorted_evdev_keys(keys)
        assert set(result) == {"KEY_A", "KEY_B"}

    def test_empty(self):
        assert sorted_evdev_keys([]) == []

    def test_single_key(self):
        assert sorted_evdev_keys(["KEY_SPACE"]) == ["KEY_SPACE"]

    def test_right_modifier_after_left(self):
        keys = ["KEY_RIGHTMETA", "KEY_LEFTMETA"]
        result = sorted_evdev_keys(keys)
        assert result.index("KEY_LEFTMETA") < result.index("KEY_RIGHTMETA")

    def test_chord_layout_held_plus_trigger(self):
        # Typical chord: Ctrl held, V pressed
        keys = ["KEY_V", "KEY_LEFTCTRL"]
        result = sorted_evdev_keys(keys)
        assert result == ["KEY_LEFTCTRL", "KEY_V"]

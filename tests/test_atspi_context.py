"""Tests for the AT-SPI2 accessibility integration module (src/atspi_context.py).

All tests use unittest.mock to simulate pyatspi so they run regardless of
whether the library is installed on the test host.
"""
import os
import sys
import threading
import types
import unittest
from unittest.mock import MagicMock, patch, PropertyMock

sys.path.insert(0, os.path.join(os.path.dirname(__file__), '..', 'src'))


# ---------------------------------------------------------------------------
# Helpers to build a minimal fake pyatspi module
# ---------------------------------------------------------------------------

def _make_pyatspi_stub():
    """Return a minimal pyatspi stub with a controllable Registry."""
    stub = types.ModuleType("pyatspi")

    class _Registry:
        _listeners: dict = {}
        _started = False

        @classmethod
        def registerEventListener(cls, callback, event_type):
            cls._listeners[event_type] = callback

        @classmethod
        def deregisterEventListener(cls, callback, event_type):
            cls._listeners.pop(event_type, None)

        @classmethod
        def start(cls):
            cls._started = True

        @classmethod
        def stop(cls):
            cls._started = False

    stub.Registry = _Registry
    stub.STATE_FOCUSED = 1
    return stub


def _make_accessible(app_name="TestApp", role_name="entry",
                     text_content="hello world", caret_offset=11,
                     insert_result=True):
    """Return a mock pyatspi Accessible with a queryText() interface."""
    text_iface = MagicMock()
    text_iface.caretOffset = caret_offset
    text_iface.getText = MagicMock(return_value=text_content)
    text_iface.insertText = MagicMock(return_value=insert_result)

    app = MagicMock()
    app.name = app_name

    accessible = MagicMock()
    accessible.getApplication = MagicMock(return_value=app)
    accessible.getRoleName = MagicMock(return_value=role_name)
    accessible.queryText = MagicMock(return_value=text_iface)
    return accessible, text_iface


# ---------------------------------------------------------------------------
# Re-import atspi_context with a fresh module state for each test class
# ---------------------------------------------------------------------------

def _fresh_module(pyatspi_stub=None):
    """Import atspi_context into a clean module namespace, optionally injecting pyatspi."""
    # Remove any cached version so importlib gives us a fresh one
    for key in list(sys.modules.keys()):
        if 'atspi_context' in key:
            del sys.modules[key]

    if pyatspi_stub is not None:
        sys.modules['pyatspi'] = pyatspi_stub
    else:
        sys.modules.pop('pyatspi', None)

    import atspi_context
    return atspi_context


# ===========================================================================
# Tests that run WITHOUT pyatspi installed (graceful degradation)
# ===========================================================================

class TestWithoutPyatspi:
    """All public functions must be no-ops when pyatspi is absent."""

    def setup_method(self):
        self.mod = _fresh_module(pyatspi_stub=None)

    def teardown_method(self):
        sys.modules.pop('pyatspi', None)

    def test_is_available_false(self):
        assert self.mod.is_available() is False

    def test_get_focused_context_returns_none(self):
        assert self.mod.get_focused_context() is None

    def test_inject_text_returns_false(self):
        assert self.mod.inject_text("hello") is False

    def test_start_is_noop(self):
        self.mod.start()   # must not raise
        assert self.mod._running is False

    def test_stop_is_noop(self):
        self.mod.stop()    # must not raise


# ===========================================================================
# Tests that run WITH a pyatspi stub
# ===========================================================================

class TestWithPyatspi:
    """Behaviour when pyatspi is present."""

    def setup_method(self):
        self.stub = _make_pyatspi_stub()
        self.stub.Registry._listeners.clear()
        self.stub.Registry._started = False
        self.mod = _fresh_module(pyatspi_stub=self.stub)

    def teardown_method(self):
        # Reset module state
        self.mod._running = False
        self.mod._focused = None
        sys.modules.pop('pyatspi', None)
        for key in list(sys.modules.keys()):
            if 'atspi_context' in key:
                del sys.modules[key]

    # --- is_available -------------------------------------------------------

    def test_is_available_true(self):
        assert self.mod.is_available() is True

    # --- start / stop -------------------------------------------------------

    def test_start_registers_listener(self):
        self.mod.start()
        assert 'object:state-changed:focused' in self.stub.Registry._listeners

    def test_start_sets_running(self):
        self.mod.start()
        assert self.mod._running is True

    def test_start_is_idempotent(self):
        self.mod.start()
        self.mod.start()   # second call must be a no-op
        assert self.mod._running is True

    def test_stop_deregisters_listener(self):
        self.mod.start()
        self.mod.stop()
        assert 'object:state-changed:focused' not in self.stub.Registry._listeners

    def test_stop_clears_running(self):
        self.mod.start()
        self.mod.stop()
        assert self.mod._running is False

    # --- focus event handling -----------------------------------------------

    def test_focus_gained_updates_focused(self):
        accessible, _ = _make_accessible()
        event = MagicMock()
        event.detail1 = 1
        event.source = accessible

        self.mod._on_focus_changed(event)
        assert self.mod._focused is accessible

    def test_focus_lost_does_not_clear_focused(self):
        accessible, _ = _make_accessible()
        # First set a focused widget
        gained = MagicMock()
        gained.detail1 = 1
        gained.source = accessible
        self.mod._on_focus_changed(gained)

        # Now send a "lost focus" event — _focused must stay the same
        lost = MagicMock()
        lost.detail1 = 0
        lost.source = accessible
        self.mod._on_focus_changed(lost)
        assert self.mod._focused is accessible

    # --- get_focused_context ------------------------------------------------

    def test_returns_none_when_no_focused_widget(self):
        self.mod._focused = None
        assert self.mod.get_focused_context() is None

    def test_returns_focus_context_fields(self):
        accessible, text_iface = _make_accessible(
            app_name="Firefox",
            role_name="entry",
            text_content="type something here",
            caret_offset=19,
        )
        self.mod._focused = accessible

        ctx = self.mod.get_focused_context(max_chars=500)

        assert ctx is not None
        assert ctx.app_name == "Firefox"
        assert ctx.role_name == "entry"
        assert ctx.surrounding_text == "type something here"
        assert ctx.cursor_offset == 19

    def test_surrounding_text_sliced_to_max_chars(self):
        long_text = "a" * 1000
        accessible, text_iface = _make_accessible(
            text_content=long_text[-300:],  # getText is called with (offset-max, offset)
            caret_offset=1000,
        )
        self.mod._focused = accessible

        ctx = self.mod.get_focused_context(max_chars=300)
        # getText should be called with start=max(0, 1000-300)=700, end=1000
        text_iface.getText.assert_called_once_with(700, 1000)

    def test_is_code_context_for_terminal_role(self):
        accessible, _ = _make_accessible(role_name="terminal")
        self.mod._focused = accessible
        ctx = self.mod.get_focused_context()
        assert ctx.is_code_context is True

    def test_is_code_context_false_for_entry_role(self):
        accessible, _ = _make_accessible(role_name="entry")
        self.mod._focused = accessible
        ctx = self.mod.get_focused_context()
        assert ctx.is_code_context is False

    def test_get_context_survives_querytext_failure(self):
        """get_focused_context must not raise when Text interface is unavailable."""
        accessible, _ = _make_accessible()
        accessible.queryText = MagicMock(side_effect=NotImplementedError)
        self.mod._focused = accessible

        ctx = self.mod.get_focused_context()
        assert ctx is not None
        assert ctx.surrounding_text == ""
        assert ctx.cursor_offset == 0

    def test_get_context_survives_getapplication_failure(self):
        accessible, _ = _make_accessible()
        accessible.getApplication = MagicMock(side_effect=Exception("dbus error"))
        self.mod._focused = accessible

        ctx = self.mod.get_focused_context()
        assert ctx is not None
        assert ctx.app_name == ""

    # --- inject_text --------------------------------------------------------

    def test_inject_text_returns_false_when_no_focused(self):
        self.mod._focused = None
        assert self.mod.inject_text("hello") is False

    def test_inject_text_calls_insert_at_caret(self):
        accessible, text_iface = _make_accessible(caret_offset=5, insert_result=True)
        self.mod._focused = accessible

        result = self.mod.inject_text("world")

        assert result is True
        text_iface.insertText.assert_called_once_with(5, "world", 5)

    def test_inject_text_returns_false_on_no_text_interface(self):
        accessible, _ = _make_accessible()
        accessible.queryText = MagicMock(side_effect=NotImplementedError)
        self.mod._focused = accessible

        assert self.mod.inject_text("hello") is False

    def test_inject_text_returns_false_when_insert_fails(self):
        accessible, text_iface = _make_accessible(insert_result=False)
        self.mod._focused = accessible

        assert self.mod.inject_text("hello") is False

    def test_inject_empty_string_returns_false(self):
        accessible, _ = _make_accessible()
        self.mod._focused = accessible
        assert self.mod.inject_text("") is False

    # --- thread safety -------------------------------------------------------

    def test_concurrent_focus_updates_are_safe(self):
        """Rapid concurrent focus changes must not corrupt _focused."""
        errors = []

        def spam_focus_events(accessible):
            for _ in range(200):
                event = MagicMock()
                event.detail1 = 1
                event.source = accessible
                try:
                    self.mod._on_focus_changed(event)
                except Exception as exc:
                    errors.append(exc)

        a1, _ = _make_accessible(app_name="App1")
        a2, _ = _make_accessible(app_name="App2")
        t1 = threading.Thread(target=spam_focus_events, args=(a1,))
        t2 = threading.Thread(target=spam_focus_events, args=(a2,))
        t1.start(); t2.start()
        t1.join(); t2.join()

        assert not errors
        # _focused must be one of the two valid accessibles
        assert self.mod._focused in (a1, a2)


# ===========================================================================
# FocusContext namedtuple
# ===========================================================================

class TestFocusContextNamedTuple:
    def setup_method(self):
        self.stub = _make_pyatspi_stub()
        self.mod = _fresh_module(pyatspi_stub=self.stub)

    def teardown_method(self):
        sys.modules.pop('pyatspi', None)
        for key in list(sys.modules.keys()):
            if 'atspi_context' in key:
                del sys.modules[key]

    def test_fields(self):
        ctx = self.mod.FocusContext(
            app_name="Gedit",
            role_name="text",
            surrounding_text="some text",
            cursor_offset=9,
            is_code_context=False,
        )
        assert ctx.app_name == "Gedit"
        assert ctx.role_name == "text"
        assert ctx.surrounding_text == "some text"
        assert ctx.cursor_offset == 9
        assert ctx.is_code_context is False

    def test_is_namedtuple(self):
        ctx = self.mod.FocusContext("A", "b", "c", 0, False)
        # NamedTuples support indexing
        assert ctx[0] == "A"
        assert ctx[1] == "b"

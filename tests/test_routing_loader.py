"""Tests for TOML config loader round-trips."""
import sys
import os
import tempfile
import pytest
from pathlib import Path

sys.path.insert(0, os.path.join(os.path.dirname(__file__), '..', 'src'))

from routing.loader import (
    load_targets, load_bindings, save_targets, save_bindings,
    _default_inject_target, _default_bindings,
)
from routing.models import DeliveryType, GestureType, HotkeyBinding, OutputTarget


class TestDefaultFallbacks:
    def test_load_targets_returns_default_when_no_file(self, tmp_path):
        targets = load_targets(config_dir=tmp_path)
        assert len(targets) == 1
        assert targets[0].id == 'default'
        assert targets[0].delivery == DeliveryType.INJECT

    def test_load_bindings_returns_defaults_when_no_file(self, tmp_path):
        bindings = load_bindings(config_dir=tmp_path)
        assert len(bindings) == 2
        ids = {b.id for b in bindings}
        assert 'default_hold' in ids
        assert 'default_toggle' in ids


class TestTargetsRoundTrip:
    def test_single_inject_target(self, tmp_path):
        targets = [OutputTarget(
            id='default', label='Focused Window',
            delivery=DeliveryType.INJECT,
            post_processing='default', append_newline=False,
        )]
        save_targets(targets, config_dir=tmp_path)
        loaded = load_targets(config_dir=tmp_path)
        assert len(loaded) == 1
        assert loaded[0].id == 'default'
        assert loaded[0].delivery == DeliveryType.INJECT
        assert loaded[0].append_newline is False

    def test_pipe_target_round_trip(self, tmp_path):
        targets = [OutputTarget(
            id='hermes', label='Hermes',
            delivery=DeliveryType.PIPE,
            pipe_path='/tmp/hermes.in',
            post_processing='strip_fillers',
        )]
        save_targets(targets, config_dir=tmp_path)
        loaded = load_targets(config_dir=tmp_path)
        assert loaded[0].pipe_path == '/tmp/hermes.in'
        assert loaded[0].post_processing == 'strip_fillers'

    def test_exec_target_round_trip(self, tmp_path):
        targets = [OutputTarget(
            id='claude', label='Claude Code',
            delivery=DeliveryType.EXEC,
            command='claude --print {TEXT}',
            post_processing='none',
        )]
        save_targets(targets, config_dir=tmp_path)
        loaded = load_targets(config_dir=tmp_path)
        assert loaded[0].command == 'claude --print {TEXT}'
        assert loaded[0].post_processing == 'none'

    def test_file_target_round_trip(self, tmp_path):
        targets = [OutputTarget(
            id='journal', label='Journal',
            delivery=DeliveryType.FILE,
            file_path='~/journal.md',
            file_prefix='- ',
            file_timestamp=True,
        )]
        save_targets(targets, config_dir=tmp_path)
        loaded = load_targets(config_dir=tmp_path)
        assert loaded[0].file_path == '~/journal.md'
        assert loaded[0].file_prefix == '- '
        assert loaded[0].file_timestamp is True

    def test_socket_target_round_trip(self, tmp_path):
        targets = [OutputTarget(
            id='remote', label='Remote',
            delivery=DeliveryType.SOCKET,
            socket_host='192.168.1.100',
            socket_port=9000,
        )]
        save_targets(targets, config_dir=tmp_path)
        loaded = load_targets(config_dir=tmp_path)
        assert loaded[0].socket_host == '192.168.1.100'
        assert loaded[0].socket_port == 9000

    def test_multiple_targets(self, tmp_path):
        targets = [
            OutputTarget(id='a', label='A', delivery=DeliveryType.INJECT),
            OutputTarget(id='b', label='B', delivery=DeliveryType.CLIPBOARD),
            OutputTarget(
                id='c', label='C', delivery=DeliveryType.EXEC,
                command='echo {TEXT}',
            ),
        ]
        save_targets(targets, config_dir=tmp_path)
        loaded = load_targets(config_dir=tmp_path)
        assert len(loaded) == 3
        assert [t.id for t in loaded] == ['a', 'b', 'c']


class TestBindingsRoundTrip:
    def test_hold_binding(self, tmp_path):
        bindings = [HotkeyBinding(
            id='hold1', label='Hold', keys=['KEY_LEFTMETA', 'KEY_SPACE'],
            gesture=GestureType.HOLD, target_id='default',
        )]
        save_bindings(bindings, config_dir=tmp_path)
        loaded = load_bindings(config_dir=tmp_path)
        assert loaded[0].keys == ['KEY_LEFTMETA', 'KEY_SPACE']
        assert loaded[0].gesture == GestureType.HOLD

    def test_double_tap_binding(self, tmp_path):
        bindings = [HotkeyBinding(
            id='dt1', label='DT', keys=['KEY_LEFTCTRL'],
            gesture=GestureType.DOUBLE_TAP, target_id='hermes',
            tap_ms=280, hold_threshold_ms=200,
        )]
        save_bindings(bindings, config_dir=tmp_path)
        loaded = load_bindings(config_dir=tmp_path)
        assert loaded[0].gesture == GestureType.DOUBLE_TAP
        assert loaded[0].tap_ms == 280
        assert loaded[0].target_id == 'hermes'

    def test_disabled_binding(self, tmp_path):
        bindings = [HotkeyBinding(
            id='dis1', label='Disabled', keys=['KEY_F13'],
            gesture=GestureType.HOLD, target_id='default',
            disabled=True,
        )]
        save_bindings(bindings, config_dir=tmp_path)
        loaded = load_bindings(config_dir=tmp_path)
        assert loaded[0].disabled is True


class TestAutoBackup:
    def test_backup_created_on_save(self, tmp_path):
        targets = [OutputTarget(id='x', label='X', delivery=DeliveryType.INJECT)]
        save_targets(targets, config_dir=tmp_path)
        # Save again to trigger backup of the first save
        save_targets(targets, config_dir=tmp_path)
        backups = list((tmp_path / 'backups').glob('targets.toml.*'))
        assert len(backups) >= 1

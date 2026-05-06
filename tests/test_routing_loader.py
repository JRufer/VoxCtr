"""Tests for TOML config loader round-trips."""
import sys
import os
import tempfile
import pytest
from pathlib import Path

sys.path.insert(0, os.path.join(os.path.dirname(__file__), '..', 'src'))

from routing.loader import (
    load_targets, load_bindings, save_targets, save_bindings,
    _default_inject_target, _default_bindings, _migrate_post_processing,
)
from routing.models import (
    DeliveryType, GestureType, HotkeyBinding, OutputTarget, TargetProcessingConfig,
)


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
            append_newline=False,
        )]
        save_targets(targets, config_dir=tmp_path)
        loaded = load_targets(config_dir=tmp_path)
        assert len(loaded) == 1
        assert loaded[0].id == 'default'
        assert loaded[0].delivery == DeliveryType.INJECT
        assert loaded[0].append_newline is False

    def test_pipe_target_round_trip(self, tmp_path):
        proc = TargetProcessingConfig(remove_fillers=True, spoken_punctuation=False)
        targets = [OutputTarget(
            id='hermes', label='Hermes',
            delivery=DeliveryType.PIPE,
            pipe_path='/tmp/hermes.in',
            processing=proc,
        )]
        save_targets(targets, config_dir=tmp_path)
        loaded = load_targets(config_dir=tmp_path)
        assert loaded[0].pipe_path == '/tmp/hermes.in'
        assert loaded[0].processing.remove_fillers is True
        assert loaded[0].processing.spoken_punctuation is False

    def test_exec_target_round_trip(self, tmp_path):
        proc = TargetProcessingConfig(remove_fillers=False, spoken_punctuation=False,
                                      auto_format_lists=False, apply_snippets=False,
                                      ollama_enabled=False)
        targets = [OutputTarget(
            id='claude', label='Claude Code',
            delivery=DeliveryType.EXEC,
            command='claude --print {TEXT}',
            processing=proc,
        )]
        save_targets(targets, config_dir=tmp_path)
        loaded = load_targets(config_dir=tmp_path)
        assert loaded[0].command == 'claude --print {TEXT}'
        assert loaded[0].processing.remove_fillers is False

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


class TestTargetProcessingConfig:
    def test_empty_config_has_no_any(self):
        cfg = TargetProcessingConfig()
        assert not cfg.has_any()

    def test_set_field_has_any(self):
        cfg = TargetProcessingConfig(remove_fillers=True)
        assert cfg.has_any()

    def test_to_dict_omits_none(self):
        cfg = TargetProcessingConfig(remove_fillers=True, spoken_punctuation=False)
        d = cfg.to_dict()
        assert d['remove_fillers'] is True
        assert d['spoken_punctuation'] is False
        assert 'quiet_mode' not in d
        assert 'ollama_enabled' not in d

    def test_from_dict_round_trip(self):
        original = TargetProcessingConfig(
            remove_fillers=True,
            spoken_punctuation=False,
            ollama_enabled=True,
            ollama_model='llama3.2:1b',
            ollama_mode='clean',
            ollama_prompt='Fix grammar.',
        )
        d = original.to_dict()
        loaded = TargetProcessingConfig.from_dict(d)
        assert loaded.remove_fillers is True
        assert loaded.spoken_punctuation is False
        assert loaded.ollama_enabled is True
        assert loaded.ollama_model == 'llama3.2:1b'
        assert loaded.ollama_mode == 'clean'
        assert loaded.ollama_prompt == 'Fix grammar.'
        assert loaded.quiet_mode is None  # not set → None

    def test_get_feature_warnings_no_warnings_when_global_on(self):
        cfg = TargetProcessingConfig(ollama_enabled=True)
        global_config = {'ollama_enabled': True, 'noise_suppression': False}
        warnings = cfg.get_feature_warnings(global_config)
        assert warnings == []

    def test_get_feature_warnings_ollama_disabled_globally(self):
        cfg = TargetProcessingConfig(ollama_enabled=True)
        global_config = {'ollama_enabled': False, 'noise_suppression': False}
        warnings = cfg.get_feature_warnings(global_config)
        assert any(k == 'ollama' for k, _ in warnings)

    def test_get_feature_warnings_noise_suppression_disabled_globally(self):
        cfg = TargetProcessingConfig(noise_suppression=True)
        global_config = {'ollama_enabled': False, 'noise_suppression': False}
        warnings = cfg.get_feature_warnings(global_config)
        assert any(k == 'noise_suppression' for k, _ in warnings)

    def test_get_feature_warnings_none_when_feature_not_requested(self):
        cfg = TargetProcessingConfig()  # no overrides set
        global_config = {'ollama_enabled': False, 'noise_suppression': False}
        warnings = cfg.get_feature_warnings(global_config)
        assert warnings == []


class TestProcessingConfigRoundTrip:
    def test_ollama_config_round_trip(self, tmp_path):
        proc = TargetProcessingConfig(
            ollama_enabled=True,
            ollama_model='mistral:7b',
            ollama_mode='formal',
            ollama_prompt='Rewrite professionally: {text}',
        )
        targets = [OutputTarget(
            id='formal_notes', label='Formal Notes',
            delivery=DeliveryType.FILE,
            file_path='~/formal.md',
            processing=proc,
        )]
        save_targets(targets, config_dir=tmp_path)
        loaded = load_targets(config_dir=tmp_path)
        lp = loaded[0].processing
        assert lp.ollama_enabled is True
        assert lp.ollama_model == 'mistral:7b'
        assert lp.ollama_mode == 'formal'
        assert lp.ollama_prompt == 'Rewrite professionally: {text}'

    def test_preprocessing_overrides_round_trip(self, tmp_path):
        proc = TargetProcessingConfig(
            noise_suppression=True,
            quiet_mode=True,
            atspi_context=False,
        )
        targets = [OutputTarget(
            id='quiet_target', label='Quiet Recording',
            delivery=DeliveryType.INJECT,
            processing=proc,
        )]
        save_targets(targets, config_dir=tmp_path)
        loaded = load_targets(config_dir=tmp_path)
        lp = loaded[0].processing
        assert lp.noise_suppression is True
        assert lp.quiet_mode is True
        assert lp.atspi_context is False

    def test_postprocessing_overrides_round_trip(self, tmp_path):
        proc = TargetProcessingConfig(
            remove_fillers=False,
            spoken_punctuation=True,
            auto_format_lists=False,
            apply_snippets=True,
            code_mode=True,
        )
        targets = [OutputTarget(
            id='code_target', label='Code Mode',
            delivery=DeliveryType.INJECT,
            processing=proc,
        )]
        save_targets(targets, config_dir=tmp_path)
        loaded = load_targets(config_dir=tmp_path)
        lp = loaded[0].processing
        assert lp.remove_fillers is False
        assert lp.spoken_punctuation is True
        assert lp.auto_format_lists is False
        assert lp.apply_snippets is True
        assert lp.code_mode is True

    def test_no_processing_stored_when_empty(self, tmp_path):
        """Targets with no processing overrides should not write a processing key."""
        targets = [OutputTarget(
            id='plain', label='Plain', delivery=DeliveryType.INJECT,
        )]
        save_targets(targets, config_dir=tmp_path)
        toml_text = (tmp_path / 'targets.toml').read_text()
        assert 'processing' not in toml_text

    def test_processing_inline_table_written(self, tmp_path):
        proc = TargetProcessingConfig(remove_fillers=True, ollama_enabled=False)
        targets = [OutputTarget(
            id='test', label='Test', delivery=DeliveryType.INJECT,
            processing=proc,
        )]
        save_targets(targets, config_dir=tmp_path)
        toml_text = (tmp_path / 'targets.toml').read_text()
        assert 'processing' in toml_text
        assert 'remove_fillers' in toml_text


class TestLegacyPostProcessingMigration:
    def test_migrate_none(self):
        cfg = _migrate_post_processing('none')
        assert cfg.remove_fillers is False
        assert cfg.spoken_punctuation is False
        assert cfg.ollama_enabled is False

    def test_migrate_strip_fillers(self):
        cfg = _migrate_post_processing('strip_fillers')
        assert cfg.remove_fillers is True
        assert cfg.spoken_punctuation is False

    def test_migrate_snippets_only(self):
        cfg = _migrate_post_processing('snippets_only')
        assert cfg.apply_snippets is True
        assert cfg.remove_fillers is False

    def test_migrate_ollama_only(self):
        cfg = _migrate_post_processing('ollama_only')
        assert cfg.ollama_enabled is True
        assert cfg.remove_fillers is False

    def test_migrate_default(self):
        cfg = _migrate_post_processing('default')
        # All None → inherits global
        assert not cfg.has_any()

    def test_legacy_toml_loads_and_migrates(self, tmp_path):
        """Old targets.toml with post_processing string is migrated on load."""
        (tmp_path / 'targets.toml').write_text(
            'format_version = "1.0"\n\n'
            '[[target]]\n'
            'id = "old_target"\n'
            'label = "Old"\n'
            'delivery = "inject"\n'
            'post_processing = "strip_fillers"\n'
            'append_newline = false\n'
            'send_on_release = true\n'
            'file_timestamp = false\n',
            encoding='utf-8',
        )
        targets = load_targets(config_dir=tmp_path)
        assert targets[0].processing.remove_fillers is True
        assert targets[0].processing.spoken_punctuation is False


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

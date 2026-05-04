"""Load and save targets.toml and bindings.toml under ~/.config/whisper-wayland/."""
import os
import shutil
import tomllib
from datetime import datetime, timezone
from pathlib import Path

from routing.models import (
    DeliveryType, GestureType, HotkeyBinding, OutputTarget,
)

CONFIG_DIR = Path('~/.config/whisper-wayland').expanduser()
_FORMAT_VERSION = "1.0"
_KEEP_BACKUPS = 20


# ── TOML writer (minimal: handles str/int/bool/float and arrays of tables) ──

def _toml_value(v) -> str:
    if isinstance(v, bool):
        return "true" if v else "false"
    if isinstance(v, (int, float)):
        return str(v)
    if isinstance(v, str):
        escaped = v.replace('\\', '\\\\').replace('"', '\\"').replace('\n', '\\n')
        return f'"{escaped}"'
    if isinstance(v, list):
        items = ', '.join(_toml_value(i) for i in v)
        return f'[{items}]'
    raise TypeError(f"Unsupported TOML value type: {type(v)}")


def _write_toml(data: dict, path: Path) -> None:
    lines = []
    # Top-level scalars first
    for k, v in data.items():
        if not isinstance(v, list):
            lines.append(f'{k} = {_toml_value(v)}')
    lines.append('')
    # Arrays of tables
    for k, v in data.items():
        if isinstance(v, list):
            for item in v:
                lines.append(f'[[{k}]]')
                for ik, iv in item.items():
                    lines.append(f'{ik} = {_toml_value(iv)}')
                lines.append('')
    path.write_text('\n'.join(lines), encoding='utf-8')


# ── Defaults ────────────────────────────────────────────────────────────────

def _default_inject_target() -> OutputTarget:
    return OutputTarget(
        id='default',
        label='Focused Window',
        delivery=DeliveryType.INJECT,
        post_processing='default',
        append_newline=False,
    )


def _default_bindings() -> list:
    return [
        HotkeyBinding(
            id='default_hold',
            label='Dictate (Hold)',
            keys=['KEY_LEFTMETA', 'KEY_SPACE'],
            gesture=GestureType.HOLD,
            target_id='default',
        ),
        HotkeyBinding(
            id='default_toggle',
            label='Dictate (Toggle)',
            keys=['KEY_LEFTCTRL', 'KEY_LEFTMETA', 'KEY_SPACE'],
            gesture=GestureType.TOGGLE,
            target_id='default',
        ),
    ]


# ── Parsing ──────────────────────────────────────────────────────────────────

def _parse_target(d: dict) -> OutputTarget:
    delivery = DeliveryType(d.get('delivery', 'inject'))
    return OutputTarget(
        id=d.get('id', ''),
        label=d.get('label', ''),
        delivery=delivery,
        command=d.get('command'),
        pipe_path=d.get('pipe_path'),
        socket_host=d.get('socket_host'),
        socket_port=d.get('socket_port'),
        socket_unix=d.get('socket_unix'),
        file_path=d.get('file_path'),
        file_prefix=d.get('file_prefix', ''),
        file_timestamp=d.get('file_timestamp', True),
        dbus_signal=d.get('dbus_signal'),
        post_processing=d.get('post_processing', 'default'),
        send_on_release=d.get('send_on_release', True),
        append_newline=d.get('append_newline', True),
        initial_prompt=d.get('initial_prompt'),
    )


def _parse_binding(d: dict) -> HotkeyBinding:
    gesture = GestureType(d.get('gesture', 'hold'))
    return HotkeyBinding(
        id=d.get('id', ''),
        label=d.get('label', ''),
        keys=list(d.get('keys', [])),
        gesture=gesture,
        target_id=d.get('target_id', 'default'),
        tap_ms=int(d.get('tap_ms', 250)),
        hold_threshold_ms=int(d.get('hold_threshold_ms', 200)),
        disabled=bool(d.get('disabled', False)),
    )


# ── Serialization ────────────────────────────────────────────────────────────

def _serialize_target(t: OutputTarget) -> dict:
    d: dict = {
        'id': t.id,
        'label': t.label,
        'delivery': t.delivery.value,
        'post_processing': t.post_processing,
        'append_newline': t.append_newline,
        'send_on_release': t.send_on_release,
        'file_timestamp': t.file_timestamp,
    }
    if t.command is not None:
        d['command'] = t.command
    if t.pipe_path is not None:
        d['pipe_path'] = t.pipe_path
    if t.socket_host is not None:
        d['socket_host'] = t.socket_host
    if t.socket_port is not None:
        d['socket_port'] = t.socket_port
    if t.socket_unix is not None:
        d['socket_unix'] = t.socket_unix
    if t.file_path is not None:
        d['file_path'] = t.file_path
    if t.file_prefix:
        d['file_prefix'] = t.file_prefix
    if t.dbus_signal is not None:
        d['dbus_signal'] = t.dbus_signal
    if t.initial_prompt is not None:
        d['initial_prompt'] = t.initial_prompt
    return d


def _serialize_binding(b: HotkeyBinding) -> dict:
    d: dict = {
        'id': b.id,
        'label': b.label,
        'keys': b.keys,
        'gesture': b.gesture.value,
        'target_id': b.target_id,
        'tap_ms': b.tap_ms,
        'hold_threshold_ms': b.hold_threshold_ms,
    }
    if b.disabled:
        d['disabled'] = True
    return d


# ── Backup ────────────────────────────────────────────────────────────────────

def _backup(filename: str, config_dir: Path) -> None:
    src = config_dir / filename
    if not src.exists():
        return
    ts = datetime.now(timezone.utc).strftime('%Y-%m-%dT%H:%M:%SZ')
    dst = config_dir / 'backups' / f'{filename}.{ts}'
    dst.parent.mkdir(exist_ok=True)
    shutil.copy2(src, dst)
    _prune_backups(filename, config_dir)


def _prune_backups(filename: str, config_dir: Path) -> None:
    backup_dir = config_dir / 'backups'
    if not backup_dir.exists():
        return
    backups = sorted(backup_dir.glob(f'{filename}.*'))
    for old in backups[:-_KEEP_BACKUPS]:
        try:
            old.unlink()
        except OSError:
            pass


# ── Public API ────────────────────────────────────────────────────────────────

def load_targets(config_dir: Path = CONFIG_DIR) -> list:
    path = config_dir / 'targets.toml'
    if not path.exists():
        return [_default_inject_target()]
    with open(path, 'rb') as f:
        data = tomllib.load(f)
    targets = [_parse_target(t) for t in data.get('target', [])]
    if not targets:
        return [_default_inject_target()]
    return targets


def load_bindings(config_dir: Path = CONFIG_DIR) -> list:
    path = config_dir / 'bindings.toml'
    if not path.exists():
        return _default_bindings()
    with open(path, 'rb') as f:
        data = tomllib.load(f)
    bindings = [_parse_binding(b) for b in data.get('binding', [])]
    return bindings if bindings else _default_bindings()


def save_targets(targets: list, config_dir: Path = CONFIG_DIR) -> None:
    config_dir.mkdir(mode=0o700, parents=True, exist_ok=True)
    _backup('targets.toml', config_dir)
    path = config_dir / 'targets.toml'
    _write_toml(
        {'format_version': _FORMAT_VERSION, 'target': [_serialize_target(t) for t in targets]},
        path,
    )


def save_bindings(bindings: list, config_dir: Path = CONFIG_DIR) -> None:
    config_dir.mkdir(mode=0o700, parents=True, exist_ok=True)
    _backup('bindings.toml', config_dir)
    path = config_dir / 'bindings.toml'
    _write_toml(
        {'format_version': _FORMAT_VERSION, 'binding': [_serialize_binding(b) for b in bindings]},
        path,
    )


def targets_as_dict(targets: list) -> dict:
    """Return {target_id: OutputTarget} for quick lookup."""
    return {t.id: t for t in targets}

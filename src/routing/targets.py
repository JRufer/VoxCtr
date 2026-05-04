import os
import shlex
import shutil
import socket
import stat
import subprocess

from routing.models import DeliveryResult, DeliveryType, OutputTarget, TestResult


def _env():
    env = os.environ.copy()
    if 'WAYLAND_DISPLAY' not in env:
        uid = os.getuid()
        for i in range(3):
            if os.path.exists(f"/run/user/{uid}/wayland-{i}"):
                env['WAYLAND_DISPLAY'] = f"wayland-{i}"
                break
    return env


class InjectTarget:
    def __init__(self, config: OutputTarget):
        self.config = config

    def deliver(self, text: str) -> DeliveryResult:
        if self.config.append_newline:
            text += '\n'
        env = _env()
        is_wayland = 'WAYLAND_DISPLAY' in env

        if is_wayland and shutil.which('wtype'):
            result = subprocess.run(
                ['wtype', '--', text], env=env,
                stderr=subprocess.DEVNULL,
                stdout=subprocess.DEVNULL,
            )
            if result.returncode == 0:
                return DeliveryResult(success=True, delivered_text=text)

        if shutil.which('xdotool'):
            result = subprocess.run(
                ['xdotool', 'type', '--clearmodifiers', '--delay', '12', '--', text],
                env=env, stderr=subprocess.DEVNULL,
            )
            if result.returncode == 0:
                return DeliveryResult(success=True, delivered_text=text)

        return DeliveryResult(success=False, error="No injection method available (wtype/xdotool)")

    def test(self) -> TestResult:
        if shutil.which('wtype'):
            return TestResult(reachable=True, detail="wtype found on PATH")
        if shutil.which('xdotool'):
            return TestResult(reachable=True, detail="xdotool found on PATH")
        return TestResult(reachable=False, detail="Neither wtype nor xdotool found")


class ClipboardTarget:
    def __init__(self, config: OutputTarget):
        self.config = config

    def deliver(self, text: str) -> DeliveryResult:
        if self.config.append_newline:
            text += '\n'
        env = _env()
        if shutil.which('wl-copy'):
            proc = subprocess.run(
                ['wl-copy'], input=text.encode('utf-8'),
                env=env, stderr=subprocess.DEVNULL,
            )
            if proc.returncode == 0:
                return DeliveryResult(success=True, delivered_text=text)
        try:
            import pyperclip
            pyperclip.copy(text)
            return DeliveryResult(success=True, delivered_text=text)
        except Exception as e:
            return DeliveryResult(success=False, error=str(e))

    def test(self) -> TestResult:
        if shutil.which('wl-copy'):
            return TestResult(reachable=True, detail="wl-copy found on PATH")
        return TestResult(reachable=False, detail="wl-copy not found; pyperclip may work as fallback")


class ExecTarget:
    def __init__(self, config: OutputTarget):
        self.config = config

    def deliver(self, text: str) -> DeliveryResult:
        cmd_str = self.config.command.replace('{TEXT}', text)
        try:
            subprocess.Popen(
                shlex.split(cmd_str),
                stdout=subprocess.DEVNULL,
                stderr=subprocess.PIPE,
            )
            return DeliveryResult(success=True, delivered_text=text)
        except FileNotFoundError as e:
            return DeliveryResult(success=False, error=str(e))
        except Exception as e:
            return DeliveryResult(success=False, error=str(e))

    def test(self) -> TestResult:
        if not self.config.command:
            return TestResult(reachable=False, detail="No command configured")
        binary = shlex.split(self.config.command.replace('{TEXT}', 'test'))[0]
        found = shutil.which(binary.strip()) is not None
        if found:
            return TestResult(reachable=True, detail=f"{binary} found on PATH")
        return TestResult(reachable=False, detail=f"{binary} not found on PATH")


class PipeTarget:
    def __init__(self, config: OutputTarget):
        self.config = config

    def deliver(self, text: str) -> DeliveryResult:
        if not self.config.pipe_path:
            return DeliveryResult(success=False, error="No pipe_path configured")
        path = os.path.expanduser(self.config.pipe_path)
        if not os.path.exists(path):
            return DeliveryResult(
                success=False,
                error=f"Pipe {path} does not exist. Is the agent running?",
            )
        try:
            fd = os.open(path, os.O_WRONLY | os.O_NONBLOCK)
            payload = (text + '\n').encode('utf-8')
            os.write(fd, payload)
            os.close(fd)
            return DeliveryResult(success=True, delivered_text=text)
        except OSError as e:
            return DeliveryResult(success=False, error=str(e))

    def test(self) -> TestResult:
        if not self.config.pipe_path:
            return TestResult(reachable=False, detail="No pipe_path configured")
        path = os.path.expanduser(self.config.pipe_path)
        if not os.path.exists(path):
            return TestResult(reachable=False, detail=f"FIFO {path} not found")
        try:
            is_fifo = stat.S_ISFIFO(os.stat(path).st_mode)
        except OSError:
            is_fifo = False
        if not is_fifo:
            return TestResult(reachable=False, detail=f"{path} exists but is not a FIFO")
        # Check if any process has it open for reading via /proc
        has_reader = self._has_reader(path)
        if has_reader:
            return TestResult(reachable=True, detail=f"FIFO {path} exists with active reader")
        return TestResult(reachable=True, detail=f"FIFO {path} exists (no reader detected)")

    def _has_reader(self, path: str) -> bool:
        try:
            real = os.path.realpath(path)
            for pid in os.listdir('/proc'):
                if not pid.isdigit():
                    continue
                fd_dir = f"/proc/{pid}/fd"
                try:
                    for fd in os.listdir(fd_dir):
                        try:
                            link = os.readlink(f"{fd_dir}/{fd}")
                            if link == real:
                                return True
                        except OSError:
                            pass
                except (OSError, PermissionError):
                    pass
        except Exception:
            pass
        return False


class SocketTarget:
    def __init__(self, config: OutputTarget):
        self.config = config

    def deliver(self, text: str) -> DeliveryResult:
        payload = (text + '\n').encode('utf-8')
        try:
            if self.config.socket_unix:
                with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as s:
                    s.settimeout(5.0)
                    s.connect(self.config.socket_unix)
                    s.sendall(payload)
            else:
                with socket.create_connection(
                    (self.config.socket_host, self.config.socket_port), timeout=5.0
                ) as s:
                    s.sendall(payload)
            return DeliveryResult(success=True, delivered_text=text)
        except (OSError, ConnectionRefusedError) as e:
            return DeliveryResult(success=False, error=str(e))

    def test(self) -> TestResult:
        try:
            if self.config.socket_unix:
                with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as s:
                    s.settimeout(2.0)
                    s.connect(self.config.socket_unix)
                return TestResult(reachable=True, detail=f"Unix socket {self.config.socket_unix} reachable")
            else:
                with socket.create_connection(
                    (self.config.socket_host, self.config.socket_port), timeout=2.0
                ):
                    pass
                return TestResult(
                    reachable=True,
                    detail=f"TCP {self.config.socket_host}:{self.config.socket_port} reachable",
                )
        except (OSError, ConnectionRefusedError) as e:
            return TestResult(reachable=False, detail=str(e))


class FileTarget:
    def __init__(self, config: OutputTarget):
        self.config = config

    def deliver(self, text: str) -> DeliveryResult:
        if not self.config.file_path:
            return DeliveryResult(success=False, error="No file_path configured")
        from datetime import datetime, timezone
        path = os.path.expanduser(self.config.file_path)
        try:
            parent = os.path.dirname(path)
            if parent:
                os.makedirs(parent, exist_ok=True)
            with open(path, 'a', encoding='utf-8') as f:
                line = ''
                if self.config.file_timestamp:
                    ts = datetime.now(timezone.utc).strftime('%Y-%m-%dT%H:%M:%SZ')
                    line += f'[{ts}] '
                line += self.config.file_prefix + text + '\n'
                f.write(line)
            return DeliveryResult(success=True, delivered_text=text)
        except OSError as e:
            return DeliveryResult(success=False, error=str(e))

    def test(self) -> TestResult:
        if not self.config.file_path:
            return TestResult(reachable=False, detail="No file_path configured")
        path = os.path.expanduser(self.config.file_path)
        parent = os.path.dirname(path) or '.'
        if os.access(parent, os.W_OK):
            return TestResult(reachable=True, detail=f"Parent directory {parent} is writable")
        return TestResult(reachable=False, detail=f"Cannot write to {parent}")


class DbusTarget:
    def __init__(self, config: OutputTarget):
        self.config = config

    def deliver(self, text: str) -> DeliveryResult:
        try:
            import dbus
            bus = dbus.SessionBus()
            signal_name = self.config.dbus_signal or "ai.whisperwayland.Routing.TextRouted"
            parts = signal_name.rsplit('.', 1)
            iface = parts[0] if len(parts) == 2 else signal_name
            obj_path = '/' + iface.replace('.', '/')
            obj = bus.get_object('org.freedesktop.DBus', '/org/freedesktop/DBus')
            iface_obj = dbus.Interface(obj, 'org.freedesktop.DBus')
            iface_obj.EmitSignal(iface, parts[-1] if len(parts) == 2 else signal_name, 's', [text])
            return DeliveryResult(success=True, delivered_text=text)
        except Exception as e:
            return DeliveryResult(success=False, error=str(e))

    def test(self) -> TestResult:
        try:
            import dbus  # noqa: F401
            return TestResult(reachable=True, detail="dbus-python available")
        except ImportError:
            return TestResult(reachable=False, detail="dbus-python not installed")


def build_target(config: OutputTarget):
    """Return the appropriate target implementation for a config."""
    mapping = {
        DeliveryType.INJECT:    InjectTarget,
        DeliveryType.CLIPBOARD: ClipboardTarget,
        DeliveryType.EXEC:      ExecTarget,
        DeliveryType.PIPE:      PipeTarget,
        DeliveryType.SOCKET:    SocketTarget,
        DeliveryType.FILE:      FileTarget,
        DeliveryType.DBUS:      DbusTarget,
    }
    cls = mapping.get(config.delivery)
    if cls is None:
        raise ValueError(f"Unknown delivery type: {config.delivery}")
    return cls(config)

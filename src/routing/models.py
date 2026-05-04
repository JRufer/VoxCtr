from dataclasses import dataclass, field
from enum import Enum
from typing import Optional


class GestureType(str, Enum):
    HOLD       = "hold"
    TOGGLE     = "toggle"
    DOUBLE_TAP = "double_tap"
    CHORD      = "chord"


@dataclass
class HotkeyBinding:
    id: str
    keys: list
    gesture: GestureType
    target_id: str
    tap_ms: int = 250
    hold_threshold_ms: int = 200
    label: str = ""
    disabled: bool = False


class DeliveryType(str, Enum):
    INJECT    = "inject"
    CLIPBOARD = "clipboard"
    EXEC      = "exec"
    PIPE      = "pipe"
    SOCKET    = "socket"
    FILE      = "file"
    DBUS      = "dbus"


@dataclass
class OutputTarget:
    id: str
    label: str
    delivery: DeliveryType
    command: Optional[str] = None
    pipe_path: Optional[str] = None
    socket_host: Optional[str] = None
    socket_port: Optional[int] = None
    socket_unix: Optional[str] = None
    file_path: Optional[str] = None
    file_prefix: str = ""
    file_timestamp: bool = True
    dbus_signal: Optional[str] = None
    post_processing: str = "default"
    send_on_release: bool = True
    append_newline: bool = True
    initial_prompt: Optional[str] = None


@dataclass
class DeliveryResult:
    success: bool
    error: Optional[str] = None
    delivered_text: Optional[str] = None


@dataclass
class TestResult:
    reachable: bool
    detail: str

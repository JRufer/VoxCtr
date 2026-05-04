"""OutputTargetRouter: looks up a target by ID and dispatches delivery."""
import threading
import queue

from routing.models import DeliveryResult, OutputTarget
from routing.targets import InjectTarget, build_target


class OutputTargetRouter:
    """Maps target_id → OutputTarget and calls deliver(text)."""

    def __init__(self, targets: list, inject_fallback=None):
        self._lock = threading.Lock()
        self._targets: dict = {}
        self._inject_fallback = inject_fallback  # callable(text) for legacy inject path
        self.update_targets(targets)

    def update_targets(self, targets: list) -> None:
        with self._lock:
            self._targets = {t.id: t for t in targets}

    def deliver(self, text: str, target_id: str = 'default') -> DeliveryResult:
        with self._lock:
            config: OutputTarget | None = self._targets.get(target_id)

        if config is None:
            # Unknown target — fall back to default inject
            config = self._targets.get('default')

        if config is None:
            # No targets at all — use inject fallback if available
            if self._inject_fallback and text:
                self._inject_fallback(text)
            return DeliveryResult(
                success=False,
                error=f"Target '{target_id}' not found and no default target defined",
            )

        try:
            impl = build_target(config)
            return impl.deliver(text)
        except Exception as e:
            return DeliveryResult(success=False, error=str(e))

    def test_target(self, target_id: str):
        with self._lock:
            config = self._targets.get(target_id)
        if config is None:
            from routing.models import TestResult
            return TestResult(reachable=False, detail=f"Target '{target_id}' not found")
        try:
            impl = build_target(config)
            return impl.test()
        except Exception as e:
            from routing.models import TestResult
            return TestResult(reachable=False, detail=str(e))

    def get_target(self, target_id: str) -> OutputTarget | None:
        with self._lock:
            return self._targets.get(target_id)

    def all_targets(self) -> list:
        with self._lock:
            return list(self._targets.values())

"""Tests for output target delivery implementations."""
import os
import stat
import tempfile
import threading
import sys
import pytest

sys.path.insert(0, os.path.join(os.path.dirname(__file__), '..', 'src'))

from routing.models import DeliveryType, OutputTarget
from routing.targets import ExecTarget, PipeTarget, FileTarget, ClipboardTarget, build_target


def _make_target(**kwargs) -> OutputTarget:
    defaults = dict(id='t', label='Test', delivery=DeliveryType.INJECT)
    defaults.update(kwargs)
    return OutputTarget(**defaults)


class TestExecTarget:
    def test_substitutes_text(self):
        tgt = ExecTarget(_make_target(delivery=DeliveryType.EXEC, command='echo {TEXT}'))
        result = tgt.deliver('hello world')
        assert result.success
        assert result.delivered_text == 'hello world'

    def test_nonexistent_binary(self):
        tgt = ExecTarget(_make_target(
            delivery=DeliveryType.EXEC,
            command='nonexistent_binary_xyz_abc {TEXT}',
        ))
        result = tgt.deliver('test')
        assert not result.success
        assert result.error is not None

    def test_test_method_finds_echo(self):
        tgt = ExecTarget(_make_target(delivery=DeliveryType.EXEC, command='echo {TEXT}'))
        result = tgt.test()
        assert result.reachable

    def test_test_method_missing_binary(self):
        tgt = ExecTarget(_make_target(
            delivery=DeliveryType.EXEC,
            command='definitely_missing_binary {TEXT}',
        ))
        result = tgt.test()
        assert not result.reachable


class TestPipeTarget:
    def test_delivers_to_fifo(self):
        with tempfile.TemporaryDirectory() as tmp:
            pipe_path = os.path.join(tmp, 'test.fifo')
            os.mkfifo(pipe_path)

            received = []

            def reader():
                with open(pipe_path, 'r') as f:
                    received.append(f.readline().strip())

            t = threading.Thread(target=reader, daemon=True)
            t.start()

            tgt = PipeTarget(_make_target(delivery=DeliveryType.PIPE, pipe_path=pipe_path))
            result = tgt.deliver('test command')
            t.join(timeout=2.0)

            assert result.success
            assert received == ['test command']

    def test_missing_pipe_returns_error(self):
        tgt = PipeTarget(_make_target(
            delivery=DeliveryType.PIPE,
            pipe_path='/tmp/definitely_does_not_exist_xyz.fifo',
        ))
        result = tgt.deliver('hello')
        assert not result.success
        assert 'does not exist' in result.error.lower() or 'not exist' in result.error

    def test_test_method_detects_fifo(self):
        with tempfile.TemporaryDirectory() as tmp:
            pipe_path = os.path.join(tmp, 'check.fifo')
            os.mkfifo(pipe_path)
            tgt = PipeTarget(_make_target(delivery=DeliveryType.PIPE, pipe_path=pipe_path))
            result = tgt.test()
            assert result.reachable

    def test_test_method_missing(self):
        tgt = PipeTarget(_make_target(
            delivery=DeliveryType.PIPE,
            pipe_path='/tmp/no_such_fifo_xyz.fifo',
        ))
        result = tgt.test()
        assert not result.reachable

    def test_test_method_regular_file_not_fifo(self):
        with tempfile.NamedTemporaryFile() as f:
            tgt = PipeTarget(_make_target(delivery=DeliveryType.PIPE, pipe_path=f.name))
            result = tgt.test()
            assert not result.reachable
            assert 'not a fifo' in result.detail.lower()


class TestFileTarget:
    def test_appends_with_timestamp(self):
        with tempfile.NamedTemporaryFile(mode='r', suffix='.md', delete=False) as f:
            path = f.name
        try:
            tgt = FileTarget(_make_target(
                delivery=DeliveryType.FILE,
                file_path=path,
                file_prefix='- ',
                file_timestamp=True,
            ))
            tgt.deliver('meeting note one')
            tgt.deliver('meeting note two')
            lines = open(path).readlines()
            assert len(lines) == 2
            assert '- meeting note one' in lines[0]
            assert '- meeting note two' in lines[1]
            assert lines[0].startswith('[202')  # timestamp
        finally:
            os.unlink(path)

    def test_appends_without_timestamp(self):
        with tempfile.NamedTemporaryFile(mode='r', suffix='.md', delete=False) as f:
            path = f.name
        try:
            tgt = FileTarget(_make_target(
                delivery=DeliveryType.FILE,
                file_path=path,
                file_prefix='',
                file_timestamp=False,
            ))
            tgt.deliver('hello')
            content = open(path).read()
            assert content == 'hello\n'
        finally:
            os.unlink(path)

    def test_creates_parent_dirs(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = os.path.join(tmp, 'sub', 'dir', 'notes.md')
            tgt = FileTarget(_make_target(
                delivery=DeliveryType.FILE,
                file_path=path,
                file_timestamp=False,
            ))
            result = tgt.deliver('created')
            assert result.success
            assert os.path.exists(path)

    def test_test_method_writable(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = os.path.join(tmp, 'notes.md')
            tgt = FileTarget(_make_target(delivery=DeliveryType.FILE, file_path=path))
            result = tgt.test()
            assert result.reachable


class TestBuildTarget:
    def test_build_exec(self):
        cfg = _make_target(delivery=DeliveryType.EXEC, command='echo {TEXT}')
        impl = build_target(cfg)
        assert isinstance(impl, ExecTarget)

    def test_build_pipe(self):
        cfg = _make_target(delivery=DeliveryType.PIPE, pipe_path='/tmp/test.fifo')
        impl = build_target(cfg)
        assert isinstance(impl, PipeTarget)

    def test_build_file(self):
        cfg = _make_target(delivery=DeliveryType.FILE, file_path='~/notes.md')
        impl = build_target(cfg)
        assert isinstance(impl, FileTarget)

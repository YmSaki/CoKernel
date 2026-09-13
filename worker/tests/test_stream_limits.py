from __future__ import annotations

import pytest
from IPython.core.interactiveshell import InteractiveShell

from cokernel_worker.execution import ExecutionEngine


@pytest.fixture(autouse=True)
def isolated_ipython_singleton():
    InteractiveShell.clear_instance()
    yield
    InteractiveShell.clear_instance()


def test_execution_engine_bounds_stdout_during_capture() -> None:
    engine = ExecutionEngine(max_stream_capture_bytes=64)
    outcome = engine.execute("print('x' * 10000)")

    assert outcome.success
    assert len(outcome.stdout.encode("utf-8")) <= 64
    assert outcome.stdout_truncated_bytes > 9000
    assert outcome.stderr_truncated_bytes == 0


def test_execution_engine_bounds_stderr_and_preserves_utf8() -> None:
    engine = ExecutionEngine(max_stream_capture_bytes=7)
    outcome = engine.execute("import sys\nprint('あいうえお', file=sys.stderr)")

    assert outcome.success
    assert outcome.stderr == "あい\n"
    assert len(outcome.stderr.encode("utf-8")) == 7
    assert outcome.stderr_truncated_bytes > 0


def test_execution_engine_rejects_invalid_stream_limit() -> None:
    with pytest.raises(ValueError, match="non-negative integer"):
        ExecutionEngine(max_stream_capture_bytes=-1)

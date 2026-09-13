from __future__ import annotations

import pytest
from IPython.core.interactiveshell import InteractiveShell

from cokernel_worker.execution import ExecutionEngine


@pytest.fixture(autouse=True)
def isolated_ipython_singleton():
    InteractiveShell.clear_instance()
    yield
    InteractiveShell.clear_instance()


def text_result(engine: ExecutionEngine, source: str) -> str:
    outcome = engine.execute(source)
    assert outcome.success, outcome.error
    assert outcome.final_result is not None
    return str(outcome.final_result.data["text/plain"])


def test_namespace_persists_across_cells() -> None:
    engine = ExecutionEngine()
    assert engine.execute("x = 123").success
    assert text_result(engine, "x + 1") == "124"


def test_stdout_and_stderr_are_captured() -> None:
    engine = ExecutionEngine()
    outcome = engine.execute(
        "import sys\nprint('hello-out')\nprint('hello-err', file=sys.stderr)"
    )
    assert outcome.success
    assert "hello-out" in outcome.stdout
    assert "hello-err" in outcome.stderr


def test_rich_display_mime_bundle_is_captured() -> None:
    engine = ExecutionEngine()
    outcome = engine.execute(
        "from IPython.display import HTML, display\ndisplay(HTML('<b>hello</b>'))"
    )
    assert outcome.success
    assert any(output.data.get("text/html") == "<b>hello</b>" for output in outcome.displays)


def test_top_level_await_uses_ipython_semantics() -> None:
    engine = ExecutionEngine()
    result = text_result(engine, "import asyncio\nawait asyncio.sleep(0)\n40 + 2")
    assert result == "42"


def test_python_error_is_returned_as_structured_failure() -> None:
    engine = ExecutionEngine()
    outcome = engine.execute("raise ValueError('boom')")
    assert not outcome.success
    assert outcome.error is not None
    assert outcome.error.name == "ValueError"
    assert outcome.error.value == "boom"
    assert any("ValueError: boom" in line for line in outcome.error.traceback)


def test_reset_discards_live_namespace() -> None:
    engine = ExecutionEngine()
    assert engine.execute("x = 123").success
    engine.reset()
    outcome = engine.execute("x")
    assert not outcome.success
    assert outcome.error is not None
    assert outcome.error.name == "NameError"

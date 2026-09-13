from __future__ import annotations

import base64
import json

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


def test_binary_rich_mime_is_base64_normalized_for_wire_and_ipynb() -> None:
    engine = ExecutionEngine()
    raw_png = b"\x89PNG\r\n\x1a\nCoKernel-binary"
    outcome = engine.execute(
        "from IPython.display import display\n"
        "class BinaryPng:\n"
        "    def _repr_png_(self):\n"
        "        return b'\\x89PNG\\r\\n\\x1a\\nCoKernel-binary'\n"
        "value = BinaryPng()\n"
        "display(value)\n"
        "value"
    )
    assert outcome.success, outcome.error
    assert outcome.displays
    assert outcome.final_result is not None

    explicit_png = outcome.displays[0].data["image/png"]
    result_png = outcome.final_result.data["image/png"]
    assert type(explicit_png) is str
    assert type(result_png) is str
    assert base64.b64decode(explicit_png) == raw_png
    assert base64.b64decode(result_png) == raw_png

    # Both paths must already be JSON-safe before the worker output-budget and
    # framed transport layers see the MIME bundle.
    json.dumps(outcome.displays[0].data, ensure_ascii=False)
    json.dumps(outcome.final_result.data, ensure_ascii=False)


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

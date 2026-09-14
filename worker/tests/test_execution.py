from __future__ import annotations

import base64
import json

import pytest
from IPython.core.interactiveshell import InteractiveShell

from cokernel_worker.execution import ExecutionEngine, _normalize_mime_data


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


def test_final_expression_is_not_duplicated_as_stdout() -> None:
    engine = ExecutionEngine()
    outcome = engine.execute("40 + 2")
    assert outcome.success, outcome.error
    assert outcome.stdout == ""
    assert outcome.final_result is not None
    assert outcome.final_result.data["text/plain"] == "42"


def test_final_expression_rich_repr_runs_once_and_preserves_history() -> None:
    engine = ExecutionEngine()
    outcome = engine.execute(
        "class CountingRepr:\n"
        "    calls = 0\n"
        "    def _repr_html_(self):\n"
        "        print('repr-side-effect')\n"
        "        type(self).calls += 1\n"
        "        return f'<b>{type(self).calls}</b>'\n"
        "value = CountingRepr()\n"
        "value"
    )
    assert outcome.success, outcome.error
    assert engine.shell.user_ns["CountingRepr"].calls == 1
    assert outcome.stdout == "repr-side-effect\n"
    assert outcome.final_result is not None
    assert outcome.final_result.data["text/html"] == "<b>1</b>"
    assert engine.shell.user_ns["_"] is engine.shell.user_ns["value"]
    assert outcome.execution_count in engine.shell.user_ns["Out"]
    assert engine.shell.user_ns["Out"][outcome.execution_count] is engine.shell.user_ns["value"]


def test_semicolon_suppresses_final_expression_without_stdout_leak() -> None:
    engine = ExecutionEngine()
    outcome = engine.execute("42;")
    assert outcome.success, outcome.error
    assert outcome.stdout == ""
    assert outcome.final_result is None


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

    json.dumps(outcome.displays[0].data, ensure_ascii=False)
    json.dumps(outcome.final_result.data, ensure_ascii=False)


def test_bytes_like_binary_mime_values_are_base64_normalized() -> None:
    raw = b"CoKernel-bytes-like"
    normalized = _normalize_mime_data(
        {
            "image/png": bytearray(raw),
            "application/pdf": memoryview(raw),
        }
    )

    assert base64.b64decode(normalized["image/png"]) == raw
    assert base64.b64decode(normalized["application/pdf"]) == raw
    json.dumps(normalized, ensure_ascii=False)


def test_svg_bytes_are_utf8_text_normalized_for_notebook_json() -> None:
    svg = "<svg xmlns='http://www.w3.org/2000/svg'><text>界</text></svg>"
    normalized = _normalize_mime_data({"image/svg+xml": svg.encode("utf-8")})

    assert normalized["image/svg+xml"] == svg
    json.dumps(normalized, ensure_ascii=False)


def test_invalid_utf8_svg_bytes_fail_closed() -> None:
    with pytest.raises(UnicodeDecodeError):
        _normalize_mime_data({"image/svg+xml": b"<svg>\xff</svg>"})


def test_invalid_rich_output_becomes_structured_failure_and_session_survives() -> None:
    engine = ExecutionEngine()
    outcome = engine.execute(
        "from IPython.display import display\n"
        "display({'image/svg+xml': b'<svg>\\xff</svg>'}, raw=True)\n"
        "marker = 41"
    )

    assert not outcome.success
    assert outcome.error is not None
    assert outcome.error.name == "CoKernelOutputNormalizationError"
    assert "utf-8" in outcome.error.value
    assert outcome.displays == []
    assert outcome.final_result is None

    # Output serialization failure must not destroy the persistent worker
    # namespace or poison subsequent execution.
    assert text_result(engine, "marker + 1") == "42"


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


def test_unnamed_exception_is_normalized_and_session_survives() -> None:
    engine = ExecutionEngine()
    outcome = engine.execute(
        "class NamelessError(Exception):\n"
        "    pass\n"
        "NamelessError.__name__ = ''\n"
        "marker = 41\n"
        "raise NamelessError('boom')"
    )

    assert not outcome.success
    assert outcome.error is not None
    assert outcome.error.name == "Exception"
    assert outcome.error.value == "boom"
    assert text_result(engine, "marker + 1") == "42"


def test_reset_discards_live_namespace() -> None:
    engine = ExecutionEngine()
    assert engine.execute("x = 123").success
    engine.reset()
    outcome = engine.execute("x")
    assert not outcome.success
    assert outcome.error is not None
    assert outcome.error.name == "NameError"

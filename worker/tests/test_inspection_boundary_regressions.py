"""Regression coverage for P3-INSP-03 and P3-INSP-04 acceptance boundaries."""
from __future__ import annotations

from dataclasses import asdict
import json
from typing import Any

import pytest

from cokernel_worker.inspection import (
    MAX_INTEROPERABLE_JSON_INTEGER,
    InspectionError,
    InspectionLimits,
    get_variable,
    list_variables,
)


@pytest.mark.parametrize("entrypoint", ["get", "list"])
@pytest.mark.parametrize("shape", ["direct", "list", "tuple", "dict"])
@pytest.mark.parametrize("comparison", ["raise", "true", "false"])
def test_inspection_type_dispatch_never_calls_metaclass_hooks(
    entrypoint: str, shape: str, comparison: str
) -> None:
    calls: list[str] = []

    class HostileMeta(type):
        def __eq__(cls, other: Any) -> bool:
            calls.append("eq")
            if comparison == "raise":
                raise AssertionError("metaclass equality executed")
            return comparison == "true"

        def __ne__(cls, other: Any) -> bool:
            calls.append("ne")
            raise AssertionError("metaclass inequality executed")

        def __hash__(cls) -> int:
            calls.append("hash")
            raise AssertionError("metaclass hashing executed")

    class Hostile(metaclass=HostileMeta):
        def items(self) -> Any:
            calls.append("items")
            raise AssertionError("custom container behavior executed")

    value: Any = Hostile()
    if shape == "list":
        value = [value]
    elif shape == "tuple":
        value = (value,)
    elif shape == "dict":
        value = {"nested": value}
    namespace = {"value": value}

    if entrypoint == "get":
        result = get_variable(namespace, "value")
        assert result.supported is False
        assert result.value is None
    else:
        result = list_variables(namespace)[0]
        assert result.supported is False
    assert calls == []
    # A rejected value must not poison subsequent inspection of the namespace.
    namespace["marker"] = 42
    assert get_variable(namespace, "marker").value == 42


@pytest.mark.parametrize(
    "value",
    [42, "界" * 20, [1, 2], object(), {"nested": object()},
     MAX_INTEROPERABLE_JSON_INTEGER + 1],
    ids=["integer", "utf8", "list", "unsupported", "nested-unsupported", "large-int"],
)
def test_get_variable_budget_matches_complete_protocol_result(value: Any) -> None:
    namespace = {"value": value}
    original = get_variable(namespace, "value")
    # WorkerLoop sends asdict(value), including reason=None for supported data.
    payload = json.dumps(
        asdict(original), ensure_ascii=False, allow_nan=False, separators=(",", ":")
    ).encode("utf-8")
    exact_limit = len(payload)

    accepted = get_variable(
        namespace, "value", limits=InspectionLimits(max_response_bytes=exact_limit)
    )
    assert asdict(accepted) == asdict(original)
    with pytest.raises(InspectionError, match="maximum serialized response size"):
        get_variable(
            namespace, "value",
            limits=InspectionLimits(max_response_bytes=exact_limit - 1),
        )
    if original.supported:
        listed = list_variables(
            namespace, limits=InspectionLimits(max_response_bytes=exact_limit - 1)
        )
        assert listed[0].supported is False


@pytest.mark.parametrize("base", [object, list, dict, str])
@pytest.mark.parametrize("nested", [False, True])
@pytest.mark.parametrize("entrypoint", ["get", "list"])
def test_hostile_object_and_builtin_subclass_corpus(
    base: type, nested: bool, entrypoint: str
) -> None:
    calls: list[str] = []

    def forbidden(self: Any, *args: Any, **kwargs: Any) -> Any:
        calls.append("hook")
        raise AssertionError("user hook executed during safe inspection")

    hooks = {
        name: forbidden for name in (
            "__repr__", "__str__", "__iter__", "__getitem__", "__bool__",
            "__getattr__", "__getattribute__", "__eq__", "__lt__", "__add__",
        )
    }
    hostile_type = type("HostileValue", (base,), hooks)
    value = hostile_type()
    namespace = {"value": [value] if nested else value}
    if entrypoint == "get":
        assert get_variable(namespace, "value").supported is False
    else:
        assert list_variables(namespace)[0].supported is False
    assert calls == []


@pytest.mark.parametrize("max_depth", [0, 1, 8])
def test_inspection_depth_boundary_and_support_probe_agree(max_depth: int) -> None:
    limits = InspectionLimits(max_depth=max_depth)
    value: Any = 42
    for _ in range(max_depth):
        value = [value]
    assert get_variable({"value": value}, "value", limits=limits).supported is True
    too_deep = {"value": [value]}
    with pytest.raises(InspectionError, match="maximum nesting depth"):
        get_variable(too_deep, "value", limits=limits)
    assert list_variables(too_deep, limits=limits)[0].supported is False


@pytest.mark.parametrize(
    "namespace",
    [{}, {"value": object()}, {"a": object(), "b": object()}, {"変数": object()}],
    ids=["empty", "one", "multiple", "utf8-name"],
)
def test_list_budget_includes_protocol_variables_wrapper(
    namespace: dict[str, Any],
) -> None:
    # Unsupported objects keep support flags stable while varying the budget.
    original = list_variables(namespace)
    payload = json.dumps(
        {"variables": [asdict(item) for item in original]},
        ensure_ascii=False, allow_nan=False, separators=(",", ":"),
    ).encode("utf-8")
    exact_limit = len(payload)
    accepted = list_variables(
        namespace, limits=InspectionLimits(max_response_bytes=exact_limit)
    )
    assert [asdict(item) for item in accepted] == [asdict(item) for item in original]
    with pytest.raises(InspectionError, match="maximum serialized response size"):
        list_variables(
            namespace, limits=InspectionLimits(max_response_bytes=exact_limit - 1)
        )

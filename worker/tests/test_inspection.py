from __future__ import annotations

import math

import pytest

from cokernel_worker.inspection import (
    InspectionError,
    InspectionLimits,
    get_variable,
    list_variables,
)


class Hostile:
    touched = False

    def _touch(self):
        type(self).touched = True
        raise AssertionError("user-controlled behavior must not execute")

    def __repr__(self):
        return self._touch()

    def __str__(self):
        return self._touch()

    def __iter__(self):
        return self._touch()

    def __getattr__(self, name):
        return self._touch()


class HostileList(list):
    def __iter__(self):
        raise AssertionError("list subclass iteration must not execute")


def test_get_variable_supports_bounded_exact_builtin_json_types() -> None:
    result = get_variable(
        {"value": {"a": [1, True, None, 2.5, "x"], "b": (3, 4)}},
        "value",
    )
    assert result.supported is True
    assert result.value == {"a": [1, True, None, 2.5, "x"], "b": [3, 4]}


def test_get_variable_rejects_expressions_and_keywords() -> None:
    for name in ["a.b", "a[0]", "f()", "x + 1", "import", ""]:
        with pytest.raises(InspectionError):
            get_variable({"a": 1}, name)


def test_get_variable_reports_custom_object_without_invoking_behavior() -> None:
    Hostile.touched = False
    result = get_variable({"obj": Hostile()}, "obj")
    assert result.supported is False
    assert result.value is None
    assert result.type_name == "Hostile"
    assert Hostile.touched is False


def test_get_variable_rejects_builtin_subclasses_without_iteration() -> None:
    result = get_variable({"items": HostileList([1, 2])}, "items")
    assert result.supported is False


def test_get_variable_rejects_cycles_non_finite_values_and_limits() -> None:
    cyclic = []
    cyclic.append(cyclic)
    with pytest.raises(InspectionError, match="cyclic"):
        get_variable({"x": cyclic}, "x")
    with pytest.raises(InspectionError, match="non-finite"):
        get_variable({"x": math.inf}, "x")
    with pytest.raises(InspectionError, match="maximum item count"):
        get_variable(
            {"x": [1, 2, 3]},
            "x",
            limits=InspectionLimits(max_items=2),
        )
    with pytest.raises(InspectionError, match="maximum length"):
        get_variable(
            {"x": "abc"},
            "x",
            limits=InspectionLimits(max_string_chars=2),
        )


def test_list_variables_avoids_repr_and_marks_support() -> None:
    Hostile.touched = False
    result = {
        item.name: item
        for item in list_variables(
            {"good": 1, "obj": Hostile(), "_private": 2, "not valid": 3}
        )
    }
    assert result["good"].supported is True
    assert result["obj"].supported is False
    assert "_private" not in result
    assert Hostile.touched is False

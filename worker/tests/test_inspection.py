from __future__ import annotations

import math

import pytest

from cokernel_worker.inspection import (
    MAX_INTEROPERABLE_JSON_INTEGER,
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

    def __eq__(self, other):
        return self._touch()

    def __add__(self, other):
        return self._touch()


class HostileList(list):
    def __iter__(self):
        raise AssertionError("list subclass iteration must not execute")


class HostileMapping(dict):
    touched = False

    def _touch(self):
        type(self).touched = True
        raise AssertionError("namespace behavior must not execute")

    def keys(self):
        return self._touch()

    def items(self):
        return self._touch()

    def __contains__(self, key):
        return self._touch()

    def __getitem__(self, key):
        return self._touch()


class CollidingKey:
    touched = False

    def __hash__(self):
        return hash("target")

    def __eq__(self, other):
        type(self).touched = True
        return False


def test_get_variable_supports_bounded_exact_builtin_json_types() -> None:
    result = get_variable(
        {"value": {"a": [1, True, None, 2.5, "x"], "b": (3, 4)}},
        "value",
    )
    assert result.supported is True
    assert result.value == {"a": [1, True, None, 2.5, "x"], "b": [3, 4]}


def test_get_variable_supports_interoperable_json_integer_boundaries() -> None:
    for value in [
        -MAX_INTEROPERABLE_JSON_INTEGER,
        MAX_INTEROPERABLE_JSON_INTEGER,
    ]:
        result = get_variable({"value": value}, "value")
        assert result.supported is True
        assert result.value == value


def test_get_variable_marks_out_of_range_integer_unsupported() -> None:
    value = MAX_INTEROPERABLE_JSON_INTEGER + 1
    listed = {item.name: item for item in list_variables({"value": value})}
    assert listed["value"].supported is False

    result = get_variable({"value": value}, "value")
    assert result.supported is False
    assert result.value is None
    assert result.reason == "integer value exceeds interoperable JSON safe range"

    nested = get_variable({"value": {"nested": [value]}}, "value")
    assert nested.supported is False
    assert nested.value is None
    assert nested.reason == "integer value exceeds interoperable JSON safe range"


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


def test_inspection_rejects_mapping_subclasses_without_invoking_them() -> None:
    HostileMapping.touched = False
    namespace = HostileMapping({"x": 1})
    with pytest.raises(InspectionError, match="exact dict"):
        list_variables(namespace)
    with pytest.raises(InspectionError, match="exact dict"):
        get_variable(namespace, "x")
    assert HostileMapping.touched is False


def test_get_variable_does_not_compare_non_string_namespace_keys() -> None:
    hostile_key = CollidingKey()
    namespace = {hostile_key: "ignored", "target": 123}
    CollidingKey.touched = False

    result = get_variable(namespace, "target")

    assert result.value == 123
    assert CollidingKey.touched is False


def test_inspection_rejects_oversized_namespaces_before_scanning_entries() -> None:
    hostile_key = CollidingKey()
    namespace = {hostile_key: "ignored", "target": 123}
    CollidingKey.touched = False
    limits = InspectionLimits(max_namespace_items=1)

    with pytest.raises(InspectionError, match="namespace exceeds maximum item count"):
        list_variables(namespace, limits=limits)
    with pytest.raises(InspectionError, match="namespace exceeds maximum item count"):
        get_variable(namespace, "target", limits=limits)

    assert CollidingKey.touched is False


def test_inspection_rejects_invalid_namespace_item_limit() -> None:
    limits = InspectionLimits(max_namespace_items=-1)
    with pytest.raises(InspectionError, match="max_namespace_items"):
        list_variables({}, limits=limits)
    with pytest.raises(InspectionError, match="max_namespace_items"):
        get_variable({}, "x", limits=limits)


def test_list_variables_enforces_response_size_limit() -> None:
    namespace = {f"value_{index}": index for index in range(20)}
    with pytest.raises(InspectionError, match="maximum serialized response size"):
        list_variables(namespace, limits=InspectionLimits(max_response_bytes=128))


def test_type_metadata_never_stringifies_user_controlled_values() -> None:
    class OddMetadata:
        pass

    original = OddMetadata.__module__
    try:
        OddMetadata.__module__ = Hostile()
        Hostile.touched = False
        result = get_variable({"x": OddMetadata()}, "x")
        assert result.supported is False
        assert result.type_module == "<unknown>"
        assert result.type_name == "<unknown>"
        assert Hostile.touched is False
    finally:
        OddMetadata.__module__ = original

from __future__ import annotations

import math

from cokernel_worker.inspection import (
    InspectionLimits,
    MAX_INTEROPERABLE_JSON_INTEGER,
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


def test_list_variables_supported_matches_nested_get_contract() -> None:
    cyclic: list[object] = []
    cyclic.append(cyclic)
    huge = MAX_INTEROPERABLE_JSON_INTEGER + 1
    Hostile.touched = False
    namespace = {
        "good": [1, {"nested": ("x", True)}],
        "custom_nested": [Hostile()],
        "huge_nested": {"value": huge},
        "nonfinite_nested": [math.inf],
        "cyclic": cyclic,
    }

    listed = {item.name: item for item in list_variables(namespace)}

    assert listed["good"].supported is True
    for name in ["custom_nested", "huge_nested", "nonfinite_nested", "cyclic"]:
        assert listed[name].supported is False
    assert Hostile.touched is False


def test_list_variables_supported_accounts_for_get_response_bound() -> None:
    limits = InspectionLimits(max_response_bytes=160)
    listed = {
        item.name: item
        for item in list_variables({"value": "x" * 120}, limits=limits)
    }
    assert listed["value"].supported is False


def test_list_variables_support_probe_does_not_touch_nested_hostile_object() -> None:
    Hostile.touched = False
    listed = {
        item.name: item
        for item in list_variables({"value": {"nested": Hostile()}})
    }
    assert listed["value"].supported is False
    assert Hostile.touched is False

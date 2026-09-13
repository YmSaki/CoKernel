from __future__ import annotations

import pytest

from cokernel_worker.inspection import InspectionError, InspectionLimits, get_variable


class CollidingKey:
    touched = False

    def __hash__(self):
        return hash("target")

    def __eq__(self, other):
        type(self).touched = True
        return False


def test_get_variable_rejects_oversized_identifier_before_namespace_scan() -> None:
    hostile_key = CollidingKey()
    namespace = {hostile_key: "ignored", "target": 123}
    CollidingKey.touched = False

    with pytest.raises(InspectionError, match="variable name exceeds maximum length"):
        get_variable(namespace, "target", limits=InspectionLimits(max_string_chars=3))

    assert CollidingKey.touched is False


def test_get_variable_rejects_invalid_string_limit_explicitly() -> None:
    with pytest.raises(InspectionError, match="max_string_chars"):
        get_variable({}, "x", limits=InspectionLimits(max_string_chars=-1))

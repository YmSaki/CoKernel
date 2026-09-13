from __future__ import annotations

from dataclasses import dataclass
import json
import keyword
import math
from typing import Any, Mapping


class InspectionError(ValueError):
    pass


class UnsupportedValue(InspectionError):
    pass


@dataclass(frozen=True, slots=True)
class InspectionLimits:
    max_depth: int = 8
    max_items: int = 1024
    max_string_chars: int = 65536
    max_response_bytes: int = 1_048_576


@dataclass(slots=True)
class VariableSummary:
    name: str
    type_module: str
    type_name: str
    supported: bool


@dataclass(slots=True)
class VariableValue:
    name: str
    type_module: str
    type_name: str
    supported: bool
    value: Any | None
    reason: str | None = None


def _require_exact_namespace(namespace: Mapping[str, Any]) -> dict[str, Any]:
    if type(namespace) is not dict:
        raise InspectionError("inspection namespace must be an exact dict")
    return namespace


def _is_supported_exact_type(value: Any) -> bool:
    return type(value) in (type(None), bool, int, float, str, list, tuple, dict)


def _type_metadata(value: Any, *, max_string_chars: int) -> tuple[str, str]:
    value_type = type(value)
    module = type.__getattribute__(value_type, "__module__")
    type_name = type.__getattribute__(value_type, "__qualname__")
    if type(module) is not str or type(type_name) is not str:
        return "<unknown>", "<unknown>"
    if len(module) > max_string_chars or len(type_name) > max_string_chars:
        raise InspectionError("variable type metadata exceeds maximum length")
    return module, type_name


def validate_identifier(name: str) -> None:
    if type(name) is not str or not name.isidentifier() or keyword.iskeyword(name):
        raise InspectionError("variable name must be one non-keyword Python identifier")


def list_variables(
    namespace: Mapping[str, Any],
    *,
    max_items: int = 512,
    limits: InspectionLimits | None = None,
) -> list[VariableSummary]:
    namespace = _require_exact_namespace(namespace)
    limits = limits or InspectionLimits()
    if type(max_items) is not int or max_items < 0:
        raise InspectionError("max_items must be a non-negative integer")
    item_limit = min(max_items, limits.max_items)

    entries = [
        (name, value)
        for name, value in namespace.items()
        if type(name) is str
        and len(name) <= limits.max_string_chars
        and name.isidentifier()
        and not keyword.iskeyword(name)
        and not name.startswith("_")
    ]
    entries.sort(key=lambda item: item[0])

    result: list[VariableSummary] = []
    for name, value in entries[:item_limit]:
        module, type_name = _type_metadata(
            value, max_string_chars=limits.max_string_chars
        )
        result.append(
            VariableSummary(
                name=name,
                type_module=module,
                type_name=type_name,
                supported=_is_supported_exact_type(value),
            )
        )

    encoded = json.dumps(
        [
            {
                "name": item.name,
                "type_module": item.type_module,
                "type_name": item.type_name,
                "supported": item.supported,
            }
            for item in result
        ],
        ensure_ascii=False,
        allow_nan=False,
        separators=(",", ":"),
    ).encode("utf-8")
    if len(encoded) > limits.max_response_bytes:
        raise InspectionError("variable list exceeds maximum serialized response size")
    return result


def get_variable(
    namespace: Mapping[str, Any],
    name: str,
    *,
    limits: InspectionLimits | None = None,
) -> VariableValue:
    namespace = _require_exact_namespace(namespace)
    validate_identifier(name)
    limits = limits or InspectionLimits()

    found = False
    value: Any = None
    for key, candidate in namespace.items():
        if type(key) is str and key == name:
            value = candidate
            found = True
            break
    if not found:
        raise InspectionError(f"variable not found: {name}")

    module, type_name = _type_metadata(
        value, max_string_chars=limits.max_string_chars
    )
    budget = _Budget(limits.max_items)
    try:
        serialized = _serialize(value, limits=limits, budget=budget, depth=0, seen=set())
    except UnsupportedValue as error:
        return VariableValue(
            name=name,
            type_module=module,
            type_name=type_name,
            supported=False,
            value=None,
            reason=str(error),
        )

    result = VariableValue(
        name=name,
        type_module=module,
        type_name=type_name,
        supported=True,
        value=serialized,
    )
    try:
        encoded = json.dumps(
            {
                "name": result.name,
                "type_module": result.type_module,
                "type_name": result.type_name,
                "supported": result.supported,
                "value": result.value,
            },
            ensure_ascii=False,
            allow_nan=False,
            separators=(",", ":"),
        ).encode("utf-8")
    except (TypeError, ValueError, OverflowError) as error:
        raise InspectionError("variable value cannot be encoded safely") from error
    if len(encoded) > limits.max_response_bytes:
        raise InspectionError("variable value exceeds maximum serialized response size")
    return result


@dataclass(slots=True)
class _Budget:
    remaining: int

    def consume(self, amount: int = 1) -> None:
        self.remaining -= amount
        if self.remaining < 0:
            raise InspectionError("variable value exceeds maximum item count")


def _serialize(
    value: Any,
    *,
    limits: InspectionLimits,
    budget: _Budget,
    depth: int,
    seen: set[int],
) -> Any:
    if depth > limits.max_depth:
        raise InspectionError("variable value exceeds maximum nesting depth")

    value_type = type(value)
    if value_type is type(None) or value_type is bool or value_type is int:
        budget.consume()
        return value
    if value_type is float:
        budget.consume()
        if not math.isfinite(value):
            raise InspectionError("non-finite floats are not supported")
        return value
    if value_type is str:
        budget.consume()
        if len(value) > limits.max_string_chars:
            raise InspectionError("string value exceeds maximum length")
        return value

    if value_type not in (list, tuple, dict):
        module, type_name = _type_metadata(
            value, max_string_chars=limits.max_string_chars
        )
        raise UnsupportedValue(f"unsupported variable type: {module}.{type_name}")

    object_id = id(value)
    if object_id in seen:
        raise InspectionError("cyclic container values are not supported")
    seen.add(object_id)
    try:
        if value_type is list or value_type is tuple:
            budget.consume()
            return [
                _serialize(
                    item,
                    limits=limits,
                    budget=budget,
                    depth=depth + 1,
                    seen=seen,
                )
                for item in value
            ]

        budget.consume()
        output: dict[str, Any] = {}
        for key, item in value.items():
            if type(key) is not str:
                raise UnsupportedValue("dictionary keys must be exact strings")
            if len(key) > limits.max_string_chars:
                raise InspectionError("dictionary key exceeds maximum length")
            budget.consume()
            output[key] = _serialize(
                item,
                limits=limits,
                budget=budget,
                depth=depth + 1,
                seen=seen,
            )
        return output
    finally:
        seen.remove(object_id)

from __future__ import annotations

from dataclasses import dataclass, field
import json
from typing import Any, Callable

DEFAULT_MAX_OUTPUT_EVENT_BYTES = 6 * 1024 * 1024
DEFAULT_MAX_OUTPUT_OPERATION_BYTES = 24 * 1024 * 1024
DEFAULT_MAX_OUTPUT_BLOB_BYTES = 4 * 1024 * 1024

MessageBuilder = Callable[[str, dict[str, Any]], dict[str, Any]]


@dataclass(frozen=True, slots=True)
class OutputLimits:
    max_event_bytes: int = DEFAULT_MAX_OUTPUT_EVENT_BYTES
    max_operation_bytes: int = DEFAULT_MAX_OUTPUT_OPERATION_BYTES
    max_blob_bytes: int = DEFAULT_MAX_OUTPUT_BLOB_BYTES

    def validate(self, *, max_frame_bytes: int) -> None:
        for name, value in (
            ("max_event_bytes", self.max_event_bytes),
            ("max_operation_bytes", self.max_operation_bytes),
            ("max_blob_bytes", self.max_blob_bytes),
        ):
            if type(value) is not int or value <= 0:
                raise ValueError(f"{name} must be a positive integer")
        if type(max_frame_bytes) is not int or max_frame_bytes <= 0:
            raise ValueError("max_frame_bytes must be a positive integer")
        if self.max_event_bytes >= max_frame_bytes:
            raise ValueError("max_event_bytes must be smaller than worker frame limit")


@dataclass(slots=True)
class OperationOutputBudget:
    limits: OutputLimits
    used_bytes: int = 0
    omitted_bytes: int = 0
    reasons: set[str] = field(default_factory=set)

    @property
    def remaining_bytes(self) -> int:
        return max(0, self.limits.max_operation_bytes - self.used_bytes)

    @property
    def truncated(self) -> bool:
        return self.omitted_bytes > 0

    def prepare_event(
        self,
        event: str,
        payload: dict[str, Any],
        *,
        build_message: MessageBuilder,
        source_omitted_bytes: int = 0,
        source_reason: str | None = None,
    ) -> dict[str, Any] | None:
        bounded, blob_omitted, blob_reasons = _bound_blob_mimes(
            payload, self.limits
        )
        omitted = source_omitted_bytes + blob_omitted
        reasons = set(blob_reasons)
        if source_omitted_bytes and source_reason:
            reasons.add(source_reason)

        if omitted:
            bounded = dict(bounded)
            bounded["truncation"] = _truncation_metadata(omitted, reasons)

        original_message = build_message(event, bounded)
        original_size = json_size(original_message)
        allowed = min(self.limits.max_event_bytes, self.remaining_bytes)

        if original_size <= allowed:
            self.used_bytes += original_size
            self._record_omission(omitted, reasons)
            return original_message

        limit_reason = (
            "operation_limit"
            if self.remaining_bytes < self.limits.max_event_bytes
            else "event_limit"
        )
        if event in {"stdout", "stderr"} and type(bounded.get("text")) is str:
            shrunk = _fit_stream_event(
                event,
                bounded,
                allowed,
                build_message=build_message,
                limit_reason=limit_reason,
            )
        else:
            shrunk = _minimal_output_event(
                event,
                bounded,
                allowed,
                build_message=build_message,
            )

        if shrunk is None:
            self._record_omission(original_size + omitted, reasons | {limit_reason})
            return None

        shrunk_message, dropped = shrunk
        total_omitted = omitted + dropped
        shrunk_payload = dict(shrunk_message["payload"])
        final_reasons = reasons | {limit_reason}
        shrunk_payload["truncation"] = _truncation_metadata(
            total_omitted,
            final_reasons,
        )
        shrunk_message = build_message(event, shrunk_payload)

        if json_size(shrunk_message) > allowed:
            if event in {"stdout", "stderr"} and type(shrunk_payload.get("text")) is str:
                second = _fit_stream_event(
                    event,
                    shrunk_payload,
                    allowed,
                    build_message=build_message,
                    limit_reason=limit_reason,
                )
            else:
                second = _minimal_output_event(
                    event,
                    shrunk_payload,
                    allowed,
                    build_message=build_message,
                )
            if second is None:
                self._record_omission(original_size + omitted, final_reasons)
                return None
            shrunk_message, extra_dropped = second
            total_omitted += extra_dropped

        final_size = json_size(shrunk_message)
        self.used_bytes += final_size
        self._record_omission(total_omitted, final_reasons)
        return shrunk_message

    def _record_omission(self, omitted_bytes: int, reasons: set[str]) -> None:
        if omitted_bytes <= 0:
            return
        self.omitted_bytes += omitted_bytes
        self.reasons.update(reasons or {"output_limit"})


def encode_json(value: Any) -> bytes:
    return json.dumps(
        value,
        ensure_ascii=False,
        separators=(",", ":"),
    ).encode("utf-8")


def json_size(value: Any) -> int:
    return len(encode_json(value))


def _truncate_utf8(value: str, max_bytes: int) -> tuple[str, int]:
    encoded = value.encode("utf-8", errors="replace")
    if len(encoded) <= max_bytes:
        return value, 0
    if max_bytes <= 0:
        return "", len(encoded)
    prefix = encoded[:max_bytes].decode("utf-8", errors="ignore")
    kept = len(prefix.encode("utf-8"))
    return prefix, len(encoded) - kept


def _is_blob_mime(mime: str) -> bool:
    return mime.startswith("image/") or mime in {
        "application/octet-stream",
        "application/pdf",
    }


def _bound_blob_mimes(
    payload: dict[str, Any], limits: OutputLimits
) -> tuple[dict[str, Any], int, set[str]]:
    data = payload.get("data")
    if type(data) is not dict:
        return payload, 0, set()

    bounded = dict(payload)
    bounded_data = dict(data)
    omitted = 0
    reasons: set[str] = set()
    for mime, value in list(bounded_data.items()):
        if type(mime) is not str or not _is_blob_mime(mime):
            continue
        value_bytes = json_size(value)
        if value_bytes > limits.max_blob_bytes:
            del bounded_data[mime]
            omitted += value_bytes
            reasons.add("blob_limit")
    bounded["data"] = bounded_data
    return bounded, omitted, reasons


def _truncation_metadata(omitted_bytes: int, reasons: set[str]) -> dict[str, Any]:
    return {
        "truncated": True,
        "omitted_bytes": omitted_bytes,
        "reasons": sorted(reasons),
    }


def _fit_stream_event(
    event: str,
    payload: dict[str, Any],
    allowed: int,
    *,
    build_message: MessageBuilder,
    limit_reason: str,
) -> tuple[dict[str, Any], int] | None:
    text = payload.get("text")
    if type(text) is not str or allowed <= 0:
        return None
    original_text_bytes = len(text.encode("utf-8", errors="replace"))
    low = 0
    high = original_text_bytes
    best: tuple[dict[str, Any], int] | None = None
    while low <= high:
        middle = (low + high) // 2
        prefix, omitted = _truncate_utf8(text, middle)
        candidate_payload = dict(payload)
        candidate_payload["text"] = prefix
        existing = candidate_payload.get("truncation")
        reasons: set[str] = set()
        previous_omitted = 0
        if type(existing) is dict:
            previous = existing.get("omitted_bytes", 0)
            if type(previous) is int and previous >= 0:
                previous_omitted = previous
            existing_reasons = existing.get("reasons")
            if type(existing_reasons) is list:
                reasons.update(x for x in existing_reasons if type(x) is str)
        if omitted:
            reasons.add(limit_reason)
            candidate_payload["truncation"] = _truncation_metadata(
                previous_omitted + omitted,
                reasons,
            )
        candidate = build_message(event, candidate_payload)
        if json_size(candidate) <= allowed:
            best = (candidate, omitted)
            low = middle + 1
        else:
            high = middle - 1
    return best


def _minimal_output_event(
    event: str,
    payload: dict[str, Any],
    allowed: int,
    *,
    build_message: MessageBuilder,
) -> tuple[dict[str, Any], int] | None:
    original_size = json_size(build_message(event, payload))
    minimal: dict[str, Any] = {
        "operation_id": payload.get("operation_id"),
        "sequence": payload.get("sequence"),
    }
    if event == "execute_result":
        minimal["execution_count"] = payload.get("execution_count")
        minimal["data"] = {}
        minimal["metadata"] = {}
    elif event == "display_data":
        minimal["data"] = {}
        minimal["metadata"] = {}
    elif event == "error":
        ename = payload.get("ename")
        if type(ename) is str:
            minimal["ename"], _ = _truncate_utf8(ename, 256)
        minimal["evalue"] = ""
        minimal["traceback"] = []
    existing = payload.get("truncation")
    if type(existing) is dict:
        minimal["truncation"] = existing
    candidate = build_message(event, minimal)
    candidate_size = json_size(candidate)
    if candidate_size > allowed:
        return None
    return candidate, max(0, original_size - candidate_size)

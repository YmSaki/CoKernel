from __future__ import annotations

from cokernel_worker.execution import _BoundedTextCapture


def test_bounded_capture_never_resumes_after_utf8_boundary_truncation() -> None:
    capture = _BoundedTextCapture(4)

    # The fourth byte cuts through the four-byte emoji. The valid captured
    # prefix is therefore only ``abc``. Once that logical prefix is truncated,
    # later writes must remain omitted even though one nominal byte of capacity
    # was left unused by the UTF-8 boundary.
    assert capture.write("abc😊") == 4
    assert capture.write("Z") == 1

    assert capture.getvalue() == "abc"
    assert capture.stored_bytes == 3
    assert capture.truncated_bytes == len("😊Z".encode("utf-8"))

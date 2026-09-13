from __future__ import annotations

from dataclasses import asdict
import json
import os
import sys
from typing import Any

from .execution import ExecutionEngine


def emit(payload: dict[str, Any]) -> None:
    sys.stdout.write(json.dumps(payload, ensure_ascii=False) + "\n")
    sys.stdout.flush()


def main() -> int:
    engine = ExecutionEngine()
    emit({"type": "ready", "pid": os.getpid()})

    for raw_line in sys.stdin:
        if not raw_line.strip():
            continue
        request = json.loads(raw_line)
        method = request.get("method")

        if method == "execute":
            outcome = engine.execute(
                str(request.get("source", "")),
                cell_id=request.get("cell_id"),
            )
            emit({"type": "execution", "outcome": asdict(outcome)})
        elif method == "reset":
            engine.reset()
            emit({"type": "reset", "ok": True})
        elif method == "shutdown":
            emit({"type": "shutdown", "ok": True})
            return 0
        else:
            emit({"type": "error", "message": f"unknown spike method: {method}"})

    return 0


if __name__ == "__main__":
    raise SystemExit(main())

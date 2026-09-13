from __future__ import annotations

import json
import os
from pathlib import Path
import selectors
import signal
import subprocess
import sys
import time
from typing import Any, TextIO


ROOT = Path(__file__).resolve().parents[2]


def read_json_line(process: subprocess.Popen[str], timeout: float = 8.0) -> dict[str, Any]:
    assert process.stdout is not None
    selector = selectors.DefaultSelector()
    try:
        selector.register(process.stdout, selectors.EVENT_READ)
        events = selector.select(timeout)
        if not events:
            stderr = ""
            if process.poll() is not None and process.stderr is not None:
                stderr = process.stderr.read()
            raise RuntimeError(
                f"timed out waiting for worker response; rc={process.poll()} stderr={stderr!r}"
            )
        line = process.stdout.readline()
    finally:
        selector.close()

    if not line:
        stderr = process.stderr.read() if process.stderr is not None else ""
        raise RuntimeError(f"worker closed stdout; rc={process.poll()} stderr={stderr!r}")
    return json.loads(line)


def send(process: subprocess.Popen[str], payload: dict[str, Any]) -> None:
    assert process.stdin is not None
    process.stdin.write(json.dumps(payload) + "\n")
    process.stdin.flush()


def main() -> int:
    if os.name != "posix":
        print("[interrupt-spike] SKIP: POSIX signal proof runs in Linux/WSL")
        return 0

    process = subprocess.Popen(
        [
            "uv",
            "run",
            "--project",
            str(ROOT / "worker"),
            "--",
            "python",
            "-m",
            "cokernel_worker.spike_stdio",
        ],
        cwd=ROOT,
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        bufsize=1,
    )

    try:
        ready = read_json_line(process)
        if ready.get("type") != "ready":
            raise RuntimeError(f"unexpected worker ready message: {ready!r}")

        send(process, {"method": "execute", "source": "import time\ntime.sleep(30)"})
        time.sleep(0.5)
        os.kill(process.pid, signal.SIGINT)

        interrupted = read_json_line(process)
        outcome = interrupted.get("outcome", {})
        error = outcome.get("error") or {}
        if outcome.get("success") is not False or error.get("name") != "KeyboardInterrupt":
            raise RuntimeError(f"worker did not report KeyboardInterrupt: {interrupted!r}")

        send(process, {"method": "execute", "source": "40 + 2"})
        recovered = read_json_line(process)
        recovered_outcome = recovered.get("outcome", {})
        final_result = recovered_outcome.get("final_result") or {}
        data = final_result.get("data") or {}
        if recovered_outcome.get("success") is not True or data.get("text/plain") != "42":
            raise RuntimeError(f"worker did not survive interrupt: {recovered!r}")

        send(process, {"method": "shutdown"})
        shutdown = read_json_line(process)
        if shutdown.get("type") != "shutdown":
            raise RuntimeError(f"unexpected shutdown response: {shutdown!r}")

        process.wait(timeout=5)
        if process.returncode != 0:
            raise RuntimeError(f"worker exited with {process.returncode}")

        print("[interrupt-spike] PASS")
        return 0
    finally:
        if process.poll() is None:
            process.kill()
            process.wait(timeout=5)


if __name__ == "__main__":
    raise SystemExit(main())

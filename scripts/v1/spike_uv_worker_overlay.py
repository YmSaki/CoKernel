from __future__ import annotations

import hashlib
from pathlib import Path
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parents[2]
WORKER = ROOT / "worker"

PROJECT_TOML = """\
[project]
name = "cokernel-overlay-spike"
version = "0.0.0"
requires-python = ">=3.11"
dependencies = [
  "humanize==4.10.0",
]
"""


def run(*args: str, cwd: Path | None = None) -> None:
    print("+", " ".join(args))
    subprocess.run(args, cwd=cwd, check=True)


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main() -> int:
    with tempfile.TemporaryDirectory(prefix="cokernel-uv-overlay-") as tmp:
        project = Path(tmp)
        pyproject = project / "pyproject.toml"
        pyproject.write_text(PROJECT_TOML, encoding="utf-8")
        before = digest(pyproject)

        run("uv", "sync", "--project", str(project))
        run(
            "uv",
            "run",
            "--project",
            str(project),
            "--with-editable",
            str(WORKER),
            "--",
            "python",
            "-c",
            (
                "import humanize, IPython, cokernel_worker; "
                "assert humanize.intcomma(12345) == '12,345'; "
                "print('UV_OVERLAY_OK')"
            ),
        )

        after = digest(pyproject)
        if before != after:
            raise SystemExit("worker overlay unexpectedly modified Project pyproject.toml")

    print("[overlay-spike] PASS")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

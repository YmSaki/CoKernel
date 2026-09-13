#!/usr/bin/env python3
from __future__ import annotations

import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile


ROOT = Path(__file__).resolve().parents[2]
FIXTURE_PACKAGE = ROOT / "tests" / "fixtures" / "python-package"


def run_runtime(args: list[str], env: dict[str, str]) -> object:
    command = [
        "cargo",
        "run",
        "--quiet",
        "-p",
        "cokernel-runtime",
        "--",
        *args,
    ]
    completed = subprocess.run(
        command,
        cwd=ROOT,
        env=env,
        check=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    try:
        return json.loads(completed.stdout)
    except json.JSONDecodeError as exc:
        raise RuntimeError(
            f"runtime command did not return JSON: {' '.join(command)}\n"
            f"stdout:\n{completed.stdout}\nstderr:\n{completed.stderr}"
        ) from exc


def project_python(project_root: Path) -> Path:
    if os.name == "nt":
        return project_root / ".venv" / "Scripts" / "python.exe"
    return project_root / ".venv" / "bin" / "python"


def main() -> int:
    if shutil.which("cargo") is None:
        raise SystemExit("cargo is required")
    if shutil.which("uv") is None:
        raise SystemExit("uv is required")
    if not FIXTURE_PACKAGE.is_dir():
        raise SystemExit(f"fixture package missing: {FIXTURE_PACKAGE}")

    with tempfile.TemporaryDirectory(prefix="cokernel-v1-runtime-smoke-") as temp:
        temp_root = Path(temp)
        projects_root = temp_root / "projects"
        registry = temp_root / "state" / "projects.json"
        env = os.environ.copy()
        env["COKERNEL_PROJECTS_ROOT"] = str(projects_root)
        env["COKERNEL_PROJECT_REGISTRY"] = str(registry)
        env.setdefault("UV_NO_PROGRESS", "1")

        created = run_runtime(["project", "create", "demo"], env)
        project = created["project"]
        project_id = project["project_id"]
        project_root = Path(project["root_path"])

        assert project["environment_generation"] == 1, created
        assert (project_root / "pyproject.toml").is_file()
        assert (project_root / "uv.lock").is_file()
        python = project_python(project_root)
        assert python.is_file(), python

        status = run_runtime(["project", "status", project_id], env)
        assert status["ready"] is True, status
        assert status["environment_generation"] == 1, status

        added = run_runtime(
            ["project", "add", project_id, str(FIXTURE_PACKAGE)], env
        )
        assert added["project"]["environment_generation"] == 2, added

        imported = subprocess.run(
            [
                str(python),
                "-c",
                "import cokernel_fixture_package as p; assert p.answer() == 42; print(p.answer())",
            ],
            cwd=project_root,
            env=env,
            check=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
        assert imported.stdout.strip() == "42", imported.stdout

        listed = run_runtime(["project", "list"], env)
        assert len(listed) == 1, listed
        assert listed[0]["project"]["project_id"] == project_id, listed

        forgotten = run_runtime(["project", "forget", project_id], env)
        assert forgotten["project"]["project_id"] == project_id, forgotten
        assert project_root.is_dir(), "forget must not delete Project files"
        assert run_runtime(["project", "list"], env) == []

    print("runtime Project smoke: PASS")
    return 0


if __name__ == "__main__":
    sys.exit(main())

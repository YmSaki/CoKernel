"""Actual uv build hooks and worker descendants cannot inherit control settings.

This verifies subprocess environment/profile selection, not Linux UID isolation.
All credentials are synthetic. Reports record names/booleans, never values.
"""
from __future__ import annotations

import json
import os
from pathlib import Path
import subprocess
import tempfile
import textwrap

from smoke_notebook_runtime import Bridge, ROOT, import_notebook, wait_operation, wait_package


def main():
    subprocess.run(["cargo", "build", "-q", "-p", "cokernel-runtime"], cwd=ROOT, check=True)
    with tempfile.TemporaryDirectory(prefix="ckenv-") as temporary:
        root = Path(temporary)
        package = root / "package"
        package.mkdir()
        project_root = root / "projects/proof"
        control_home = root / "control-home"
        config = control_home / ".config/uv/uv.toml"
        config.parent.mkdir(parents=True)
        config.write_text("invalid-configuration = [\n", encoding="utf-8")
        injected = root / "injected-python"
        injected.mkdir()
        injection_marker = root / "injection-ran"
        (injected / "sitecustomize.py").write_text(
            f"from pathlib import Path\nPath({str(injection_marker)!r}).touch()\n",
            encoding="utf-8",
        )
        forbidden = {
            "COKERNEL_TEST_CONTROL_SECRET": "synthetic-control-sentinel",
            "OPENAI_API_KEY": "synthetic-provider-sentinel",
            "UV_INDEX_PASSWORD": "synthetic-index-sentinel",
            "UV_CONFIG_FILE": str(config),
            "UV_PROJECT_ENVIRONMENT": str(root / "wrong-environment"),
            "PYTHONPATH": str(injected),
            "SSH_AUTH_SOCK": str(root / "control-agent.sock"),
            "HTTP_PROXY": "http://synthetic.invalid:1",
        }
        # A standalone probe is executed by real PEP 517 hooks and their children.
        probe = textwrap.dedent(f"""\
            import json, os
            from pathlib import Path
            FORBIDDEN = {list(forbidden)!r}
            PROJECT = {str(project_root)!r}
            def inspect():
                return {{
                    'inherited_names': [name for name in FORBIDDEN if name in os.environ],
                    'home_is_project': os.environ.get('HOME') == PROJECT,
                    'config_is_project': os.environ.get('XDG_CONFIG_HOME') == str(Path(PROJECT) / '.config'),
                }}
            if __name__ == '__main__':
                print(json.dumps(inspect()))
        """)
        (package / "probe.py").write_text(probe, encoding="utf-8")
        (package / "pyproject.toml").write_text(textwrap.dedent("""\
            [build-system]
            requires = []
            build-backend = "backend"
            backend-path = ["."]
            [project]
            name = "cokernel-environment-proof"
            version = "0.1.0"
        """), encoding="utf-8")
        (package / "backend.py").write_text(textwrap.dedent("""\
            import json, subprocess, sys, zipfile
            from pathlib import Path
            from probe import inspect
            ROOT = Path(__file__).parent
            def check():
                result = inspect()
                result['child'] = json.loads(subprocess.check_output([sys.executable, str(ROOT / 'probe.py')]))
                with (ROOT / 'observed.jsonl').open('a', encoding='utf-8') as output:
                    output.write(json.dumps(result) + '\\n')
            def get_requires_for_build_wheel(config_settings=None):
                check()
                return []
            def build_wheel(wheel_directory, config_settings=None, metadata_directory=None):
                check()
                name = 'cokernel_environment_proof-0.1.0-py3-none-any.whl'
                info = 'cokernel_environment_proof-0.1.0.dist-info/'
                files = {
                    'cokernel_environment_proof.py': 'answer = 42\\n',
                    info + 'METADATA': 'Metadata-Version: 2.1\\nName: cokernel-environment-proof\\nVersion: 0.1.0\\n',
                    info + 'WHEEL': 'Wheel-Version: 1.0\\nGenerator: cokernel-proof\\nRoot-Is-Purelib: true\\nTag: py3-none-any\\n',
                }
                files[info + 'RECORD'] = ''.join(path + ',,\\n' for path in [*files, info + 'RECORD'])
                with zipfile.ZipFile(Path(wheel_directory) / name, 'w') as wheel:
                    for path, content in files.items():
                        wheel.writestr(path, content)
                return name
        """), encoding="utf-8")
        environment = dict(os.environ)
        environment.update(forbidden)
        environment.update({
            "HOME": str(control_home),
            "XDG_CONFIG_HOME": str(control_home / ".config"),
            "COKERNEL_PROJECTS_ROOT": str(root / "projects"),
            "COKERNEL_PROJECT_REGISTRY": str(root / "state/projects.json"),
            "COKERNEL_WORKER_PACKAGE": str(ROOT / "worker"),
            "COKERNEL_SESSION_SOCKET_DIR": str(root / "s"),
            "COKERNEL_FAILURE_DIR": str(root / "failures"),
        })
        # Negative control: an unfiltered launch must be detected by this probe.
        import sys
        unsafe = json.loads(subprocess.check_output(
            [sys.executable, str(package / "probe.py")], env=environment,
        ))
        assert set(unsafe["inherited_names"]) == set(forbidden)
        assert injection_marker.exists()
        injection_marker.unlink()
        bridge = Bridge(environment, root)
        try:
            created = bridge.rpc("projects.create", name="proof")
            project = wait_package(bridge, created["project_id"], created)["project"]["project_id"]
            added = bridge.rpc("packages.add", project_id=project, packages=[str(package)])
            wait_package(bridge, project, added)
            observations = [json.loads(line) for line in (package / "observed.jsonl").read_text().splitlines()]
            assert observations, "real build hooks did not run"
            def assert_clean(result):
                assert result["inherited_names"] == [], result
                assert result["home_is_project"] and result["config_is_project"], result
            for result in observations:
                assert_clean(result)
                assert_clean(result["child"])
            source = (
                "import sys, os, json, subprocess\n"
                f"sys.path.insert(0, {str(package)!r})\n"
                "from probe import inspect\n"
                "import cokernel_environment_proof\n"
                "assert cokernel_environment_proof.answer == 42\n"
                "workload_observation = inspect()\n"
                f"child_observation = json.loads(subprocess.check_output([sys.executable, {str(package / 'probe.py')!r}]))\n"
                "secret_from_master = 123456789\n"
            )
            notebook = {"nbformat": 4, "nbformat_minor": 5, "metadata": {}, "cells": [
                {"cell_type": "code", "id": "a", "source": source,
                 "metadata": {}, "execution_count": None, "outputs": []},
            ]}
            document = import_notebook(bridge, project, "proof.ipynb", notebook)["document"]
            session = bridge.rpc("sessions.ensure_primary", project_id=project, notebook_id=document["notebook_id"])
            execution = bridge.rpc("sessions.execute_cell", project_id=project,
                notebook_id=document["notebook_id"], session_id=session["session_id"],
                cell_id="a", expected_revision=document["revision"])
            completed = wait_operation(bridge, project, execution["operation_id"])
            assert completed["state"] == "PERSISTED" and completed["execution_status"] == "SUCCEEDED", completed
            for name in ["workload_observation", "child_observation"]:
                assert_clean(bridge.rpc("sessions.get_variable", project_id=project,
                    session_id=session["session_id"], name=name)["value"])
            assert bridge.rpc("sessions.get_variable", project_id=project,
                session_id=session["session_id"], name="secret_from_master")["value"] == 123456789
            assert not injection_marker.exists(), "control PYTHONPATH executed in workload"
            assert not (root / "wrong-environment").exists(), "inherited uv environment override"
            bridge.rpc("bridge.shutdown")
            bridge.process.wait(timeout=10)
            assert bridge.process.returncode == 0
            print("[workload-environment-smoke] PASS: negative control detects inheritance; actual uv build hooks/children and persistent worker/child exclude control variables/profiles; package imports and saved execution succeed. Linux identity isolation remains pending.")
        finally:
            bridge.finish()


if __name__ == "__main__":
    main()

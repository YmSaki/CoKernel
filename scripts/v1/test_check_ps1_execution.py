"""Execute the Windows gate with stand-ins to prove native failures stop it."""
from __future__ import annotations

import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
POWERSHELL = shutil.which("pwsh") or shutil.which("powershell")
STAGES = [
    "python --version",
    "cargo --version",
    "uv --version",
    "python scripts/v1/validate_work_status.py --check",
    "python -m unittest discover -s scripts/v1 -p test_*.py -q",
    "python scripts/v1/validate_protocol_fixtures.py",
    "python scripts/v1/smoke_worker_source.py",
    "cargo fmt --all --check",
    "cargo check --workspace --all-targets",
    # PowerShell consumes the argument delimiter when dispatching a function stub.
    "cargo clippy --workspace --all-targets -D warnings",
    "cargo test --workspace",
    "uv sync --project worker --dev",
    "uv run --project worker cokernel-worker --version",
    "uv run --project worker pytest worker/tests",
    "python scripts/v1/smoke_worker_protocol.py",
    "python scripts/v1/spike_uv_worker_overlay.py",
    "python scripts/v1/spike_worker_interrupt.py",
    "python scripts/v1/smoke_runtime_project.py",
    "wsl.exe bash scripts/v1/check.sh",
]

HARNESS = r"""
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $false
$global:Calls = [System.Collections.Generic.List[string]]::new()
function Invoke-StandIn([string]$Command, [object[]]$Arguments) {
    $line = "$Command $($Arguments -join ' ')"
    $global:Calls.Add($line)
    $global:LASTEXITCODE = if ($line -eq $env:CK_TEST_FAIL_STAGE) { 7 } else { 0 }
}
function global:python { Invoke-StandIn 'python' $args }
function global:cargo { Invoke-StandIn 'cargo' $args }
function global:uv { Invoke-StandIn 'uv' $args }
function global:wsl.exe { Invoke-StandIn 'wsl.exe' $args }
$failed = $false
try { & $env:CK_TEST_GATE }
catch { Write-Output "CAUGHT: $_"; $failed = $true }
finally {
    ConvertTo-Json -InputObject @($global:Calls) | Set-Content -LiteralPath $env:CK_TEST_TRACE -Encoding UTF8
}
if ($failed) { exit 79 }
"""


@unittest.skipUnless(POWERSHELL, "PowerShell execution proof runs on the Windows gate")
class CheckPowerShellExecutionTests(unittest.TestCase):
    def run_gate(self, fail_stage: str) -> tuple[subprocess.CompletedProcess[str], list[str]]:
        with tempfile.TemporaryDirectory(prefix="cokernel-check-ps1-") as temporary:
            root = Path(temporary)
            harness = root / "harness.ps1"
            harness.write_text(HARNESS, encoding="utf-8")
            trace = root / "trace.json"
            env = dict(os.environ)
            env.pop("COKERNEL_WSL_DISTRO", None)
            env.update({
                "CK_TEST_FAIL_STAGE": fail_stage,
                "CK_TEST_GATE": str(ROOT / "scripts/v1/check.ps1"),
                "CK_TEST_TRACE": str(trace),
            })
            result = subprocess.run(
                [POWERSHELL, "-NoProfile", "-NonInteractive", "-File", str(harness)],
                env=env, capture_output=True, text=True, timeout=30,
            )
            calls = json.loads(trace.read_text(encoding="utf-8-sig"))
            return result, calls

    def test_every_native_failure_stops_before_next_stage_and_pass(self) -> None:
        for index, stage in enumerate(STAGES):
            with self.subTest(stage=stage):
                result, calls = self.run_gate(stage)
                self.assertEqual(result.returncode, 79, result.stdout + result.stderr)
                self.assertNotIn("[v1-check] PASS", result.stdout)
                self.assertIn("exit code 7", result.stdout)
                self.assertEqual(calls, STAGES[:index + 1])

    def test_success_requires_every_stage_including_wsl(self) -> None:
        result, calls = self.run_gate("")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(calls, STAGES)
        self.assertIn("[v1-check] PASS", result.stdout)


if __name__ == "__main__":
    unittest.main()

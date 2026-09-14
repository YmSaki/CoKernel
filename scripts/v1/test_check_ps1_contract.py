from __future__ import annotations

from pathlib import Path
import unittest


ROOT = Path(__file__).resolve().parents[2]
CHECK_PS1 = ROOT / "scripts" / "v1" / "check.ps1"


class CheckPowerShellContractTests(unittest.TestCase):
    def test_source_diagnostics_precede_native_toolchain_and_uv_gates(self) -> None:
        script = CHECK_PS1.read_text(encoding="utf-8")
        ordered = [
            "validate_work_status.py --check",
            "unittest discover -s scripts/v1 -p 'test_*.py'",
            "validate_protocol_fixtures.py",
            "smoke_worker_source.py",
            "cargo fmt --all --check",
            "cargo check --workspace --all-targets",
            "cargo clippy --workspace --all-targets -- -D warnings",
            "cargo test --workspace",
            "uv sync --project worker --dev",
            "uv run --project worker pytest",
        ]
        positions = [script.index(fragment) for fragment in ordered]
        self.assertEqual(positions, sorted(positions))

    def test_native_slice_requires_python_cargo_and_uv(self) -> None:
        script = CHECK_PS1.read_text(encoding="utf-8")
        self.assertIn("@('python', 'cargo', 'uv')", script)
        self.assertIn('throw "$RequiredCommand is required', script)

    def test_wsl_production_gate_is_mandatory(self) -> None:
        script = CHECK_PS1.read_text(encoding="utf-8")
        self.assertIn("Get-Command 'wsl.exe'", script)
        self.assertIn("throw 'wsl.exe is required", script)
        self.assertIn("@('bash', 'scripts/v1/check.sh')", script)
        self.assertIn("WSL production-path gate failed", script)

    def test_optional_distro_selection_is_preserved(self) -> None:
        script = CHECK_PS1.read_text(encoding="utf-8")
        self.assertIn("$env:COKERNEL_WSL_DISTRO", script)
        self.assertIn("'--distribution'", script)

    def test_local_gate_does_not_invoke_github_actions(self) -> None:
        script = CHECK_PS1.read_text(encoding="utf-8").lower()
        self.assertNotIn("gh workflow", script)
        self.assertNotIn("gh run", script)
        self.assertNotIn("actions/", script)


if __name__ == "__main__":
    unittest.main()

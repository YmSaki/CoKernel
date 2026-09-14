from __future__ import annotations

from pathlib import Path
import unittest


ROOT = Path(__file__).resolve().parents[2]
CHECK_WORKER = ROOT / "scripts" / "v1" / "check-worker.sh"


class CheckWorkerContractTests(unittest.TestCase):
    def test_source_diagnostics_run_before_uv_gate(self) -> None:
        script = CHECK_WORKER.read_text(encoding="utf-8")
        ordered = [
            "validate_work_status.py --check",
            "validate_protocol_fixtures.py",
            "smoke_worker_source.py",
            "command -v uv",
            "uv sync --project worker --dev",
            "uv run --project worker pytest",
            "smoke_worker_protocol.py",
            "spike_uv_worker_overlay.py",
            "spike_worker_interrupt.py",
        ]
        positions = [script.index(fragment) for fragment in ordered]
        self.assertEqual(positions, sorted(positions))

    def test_canonical_gate_keeps_uv_path_mandatory(self) -> None:
        script = CHECK_WORKER.read_text(encoding="utf-8")
        self.assertIn("uv sync --project worker --dev", script)
        self.assertIn("uv run --project worker pytest", script)
        self.assertIn("exit 127", script[script.index("command -v uv"):])

    def test_worker_gate_does_not_invoke_github_actions(self) -> None:
        script = CHECK_WORKER.read_text(encoding="utf-8").lower()
        self.assertNotIn("gh workflow", script)
        self.assertNotIn("gh run", script)
        self.assertNotIn("actions/", script)


if __name__ == "__main__":
    unittest.main()

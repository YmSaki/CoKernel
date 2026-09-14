from __future__ import annotations

import unittest

from validate_work_status import analyze

VALID = """- Total tracked work items: **2**
- DONE: **1**
- READY_VERIFY: **0**
- IN_PROGRESS: **0**
- TODO: **0**
- BLOCKED: **1**
- Ledger DONE ratio (equal-weight rows): **50.0%**
| Phase | Issue | Items | DONE | READY_VERIFY | IN_PROGRESS | TODO | BLOCKED | State |
| 3 | #29 | 2 | 1 | 0 | 0 | 0 | 1 | ACTIVE |
| ID | Work package | Work item | State | Depends on | Done when | Evidence / note |
| P3-EXE-01 | Execution | Persistent state | DONE | P12-GATE | x+1=124 | evidence/test.md |
| P3-V-01 | Verification | cargo fmt | BLOCKED | P3-EXE-01 | exit 0 | no Rust toolchain |
"""


class WorkStatusTests(unittest.TestCase):
    def test_valid_summary(self) -> None:
        result = analyze(VALID, check_summary=True)
        self.assertEqual(result["total"], 2)
        self.assertEqual(result["states"]["DONE"], 1)

    def test_duplicate_id_rejected(self) -> None:
        with self.assertRaisesRegex(ValueError, "duplicate task ID"):
            analyze(VALID + VALID.splitlines()[-1] + "\n")

    def test_unknown_state_rejected(self) -> None:
        with self.assertRaisesRegex(ValueError, "unknown state"):
            analyze(VALID.replace("| BLOCKED |", "| MAYBE |"))

    def test_missing_evidence_field_rejected(self) -> None:
        with self.assertRaisesRegex(ValueError, "seven non-empty"):
            analyze(VALID.replace("evidence/test.md", ""))

    def test_global_summary_drift_rejected(self) -> None:
        with self.assertRaisesRegex(ValueError, "summary mismatch"):
            analyze(VALID.replace("DONE: **1**", "DONE: **2**"), check_summary=True)

    def test_phase_summary_drift_rejected(self) -> None:
        with self.assertRaisesRegex(ValueError, "summary counts"):
            analyze(VALID.replace("#29 | 2 | 1", "#29 | 3 | 1"), check_summary=True)

    def test_missing_phase_summary_rejected(self) -> None:
        with self.assertRaisesRegex(ValueError, "summary set"):
            analyze("\n".join(l for l in VALID.splitlines() if "#29" not in l),
                    check_summary=True)

    def test_wrong_ratio_rejected(self) -> None:
        with self.assertRaisesRegex(ValueError, "DONE ratio"):
            analyze(VALID.replace("50.0%", "99.0%"), check_summary=True)

    def test_missing_global_count_rejected(self) -> None:
        with self.assertRaisesRegex(ValueError, "summary mismatch"):
            analyze(VALID.replace("- TODO: **0**\n", ""), check_summary=True)

    def test_empty_ledger_rejected(self) -> None:
        with self.assertRaisesRegex(ValueError, "no WBS tasks"):
            analyze("# Empty")

    def test_code_fence_rows_are_not_counted(self) -> None:
        result = analyze(VALID + "```text\n" + VALID.splitlines()[-1] + "\n```\n",
                         check_summary=True)
        self.assertEqual(result["total"], 2)


if __name__ == "__main__":
    unittest.main()

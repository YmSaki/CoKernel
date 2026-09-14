#!/usr/bin/env python3
"""Check WBS row structure and summary arithmetic, not evidence sufficiency.

This tool never marks tasks DONE and never turns a component result into a gate
PASS. Evidence review and runtime acceptance remain separate responsibilities.
"""
from __future__ import annotations

import argparse
from collections import Counter, defaultdict
import json
from pathlib import Path
import re
import sys
from typing import Any

STATES = ("DONE", "READY_VERIFY", "IN_PROGRESS", "TODO", "BLOCKED")
DEFAULT_LEDGER = Path(__file__).resolve().parents[2] / "docs/v1/WORK_STATUS.md"


def analyze(text: str, *, check_summary: bool = False) -> dict[str, Any]:
    tasks: dict[str, list[str]] = {}
    phases: dict[str, Counter[str]] = defaultdict(Counter)
    summaries: dict[str, list[int]] = {}
    fenced = False
    for number, line in enumerate(text.splitlines(), 1):
        if line.lstrip().startswith("```"):
            fenced = not fenced
            continue
        if fenced or not line.lstrip().startswith("|"):
            continue
        cells = [cell.strip() for cell in line.strip().strip("|").split("|")]
        if cells[0].startswith("P") and cells[0] != "Phase":
            if not re.fullmatch(r"P\d+(?:-[A-Z0-9]+)+", cells[0]):
                raise ValueError(f"line {number}: malformed task ID")
            if len(cells) != 7 or not all(cells):
                raise ValueError(f"line {number}: task must have seven non-empty fields")
            task_id, _, _, state, _, _, _ = cells
            if task_id in tasks:
                raise ValueError(f"duplicate task ID: {task_id}")
            if state not in STATES:
                raise ValueError(f"{task_id}: unknown state {state}")
            tasks[task_id] = cells
            phase = task_id.split("-", 1)[0][1:]
            phase = "1-2" if phase == "12" else phase
            phases[phase][state] += 1
        elif re.fullmatch(r"\d+(?:-\d+)?", cells[0]):
            if len(cells) != 9 or not all(value.isdigit() for value in cells[2:8]):
                raise ValueError(f"line {number}: malformed phase summary")
            if cells[0] in summaries:
                raise ValueError(f"duplicate phase summary: {cells[0]}")
            summaries[cells[0]] = [int(value) for value in cells[2:8]]
    if not tasks:
        raise ValueError("no WBS tasks found")
    counts = Counter(row[3] for row in tasks.values())
    result = {
        "total": len(tasks),
        "states": {state: counts[state] for state in STATES},
        "done_ratio_percent": round(100 * counts["DONE"] / len(tasks), 1),
        "phases": {
            phase: {"total": sum(group.values()), **{s: group[s] for s in STATES}}
            for phase, group in phases.items()
        },
    }
    if check_summary:
        expected = {"Total tracked work items": len(tasks), **result["states"]}
        for label, count in expected.items():
            matches = re.findall(
                rf"^- {re.escape(label)}: \*\*(\d+)\*\*$", text, re.MULTILINE
            )
            if len(matches) != 1 or int(matches[0]) != count:
                raise ValueError(f"summary mismatch for {label}: expected {count}")
        if set(summaries) != set(phases):
            raise ValueError("phase summary set does not match task phases")
        for phase, group in phases.items():
            expected_counts = [sum(group.values()), *[group[s] for s in STATES]]
            if summaries[phase] != expected_counts:
                raise ValueError(f"phase {phase}: summary counts do not match rows")
        ratios = re.findall(
            r"^- Ledger DONE ratio.*: \*\*(\d+(?:\.\d+)?)%\*\*$",
            text, re.MULTILINE,
        )
        if len(ratios) != 1 or float(ratios[0]) != result["done_ratio_percent"]:
            raise ValueError("ledger DONE ratio does not match task counts")
    return result


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("path", nargs="?", type=Path, default=DEFAULT_LEDGER)
    parser.add_argument("--check", action="store_true", help="check recorded summaries")
    parser.add_argument("--json", action="store_true", help="print computed counts as JSON")
    args = parser.parse_args()
    try:
        result = analyze(args.path.read_text(encoding="utf-8"), check_summary=args.check)
    except (OSError, ValueError) as error:
        print(f"[wbs] FAIL: {error}", file=sys.stderr)
        return 1
    if args.json:
        print(json.dumps(result, indent=2))
    else:
        counts = ", ".join(f"{key}={value}" for key, value in result["states"].items())
        print(f"[wbs] {'PASS' if args.check else 'COUNT'}: {result['total']} items; {counts}")
        print("[wbs] This is a ledger structure/count check, not runtime acceptance.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

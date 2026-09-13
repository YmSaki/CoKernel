from __future__ import annotations

import json
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
FIXTURES = ROOT / "protocol" / "fixtures"


def main() -> int:
    paths = sorted(FIXTURES.glob("*.json"))
    if not paths:
        raise SystemExit("no v1 protocol fixtures found")

    for path in paths:
        with path.open(encoding="utf-8") as handle:
            data = json.load(handle)
        if data.get("protocol") != 1:
            raise SystemExit(f"{path}: expected protocol=1")
        print(f"OK {path.relative_to(ROOT)}")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())

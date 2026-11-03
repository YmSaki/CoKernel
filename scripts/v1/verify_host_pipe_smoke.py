#!/usr/bin/env python3
"""WSL-side postcondition proof for the real Windows Named Pipe example."""
import argparse
import json
from pathlib import Path

import nbformat


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("log", type=Path)
    parser.add_argument("root_file", type=Path)
    args = parser.parse_args()
    records = [line for line in args.log.read_text(encoding="utf-8-sig").splitlines()
               if line.startswith('{"accepted_execution_survived_pipe_close"')]
    assert len(records) == 1, "expected exactly one completed pipe proof"
    proof = json.loads(records[0])
    assert proof["windows_chunked_import"] is True
    pid = proof["worker_pid"]
    assert isinstance(pid, int) and pid > 1
    assert not Path(f"/proc/{pid}").exists(), "worker survived acknowledged stop"
    root = Path(args.root_file.read_text().strip())
    notebooks = list((root / "projects").rglob("host.ipynb"))
    assert len(notebooks) == 1
    notebook = nbformat.read(notebooks[0], as_version=4)
    nbformat.validate(notebook)
    assert notebook.metadata.shutdown_flush is True
    assert notebook.cells[0].outputs[0].data["text/plain"] == "124"
    assert notebook.cells[1].attachments["dot.png"]["image/png"] == "iVBORw0KGgo="
    assert not list((root / "projects").rglob("invalid.ipynb"))
    print("PASS: imported standard .ipynb validates; output, attachment and shutdown edit persist; failed import absent; worker exited")


if __name__ == "__main__":
    main()

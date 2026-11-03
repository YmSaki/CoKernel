"""Build a development payload archive, never copy a whole user's Linux home.

Linux only; explicit built binaries + known Python distribution + worker source.
The Rust image builder revalidates paths, ownership and payload requirements.
"""
from pathlib import Path
import argparse
import tarfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path)
    parser.add_argument("--python", required=True, type=Path)
    parser.add_argument("--uv", required=True, type=Path)
    parser.add_argument("--tunnel", required=True, type=Path)
    parser.add_argument("--ca-bundle", required=True, type=Path,
                        help="Pinned public Ubuntu ca-certificates bundle; checked by Rust image builder")
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[2]
    def clean(info):
        if any(part in {".venv", "__pycache__", ".pytest_cache", ".git"}
               for part in Path(info.name).parts):
            return None
        info.uid = info.gid = 0
        info.uname = info.gname = "root"
        return info
    with args.output.open("xb") as file, tarfile.open(fileobj=file, mode="w", dereference=False) as out:
        for name in ("cokernel-runtime", "cokernel-mcp", "cokernel-bootstrap"):
            out.add(root / "target/debug" / name, f"opt/cokernel/bin/{name}", filter=clean)
        out.add(args.uv, "opt/cokernel/bin/uv", filter=clean)
        out.add(args.tunnel, "opt/cokernel/bin/tunnel-client", filter=clean)
        out.add(args.ca_bundle, "opt/cokernel/certs/ca-certificates.crt", filter=clean)
        out.add(args.python, "opt/cokernel/python", filter=clean)
        for relative in ("src", "pyproject.toml", "uv.lock"):
            out.add(root / "worker" / relative, f"opt/cokernel/worker/{relative}", filter=clean)
    print("payload archive created; not an installed-system acceptance result")


if __name__ == "__main__":
    main()

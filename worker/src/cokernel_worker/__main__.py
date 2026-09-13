from __future__ import annotations

import argparse

from . import __version__
from .protocol import connect_and_run


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="cokernel-worker")
    parser.add_argument("--version", action="store_true")
    parser.add_argument("--socket", help="Supervisor-owned Unix socket path")
    parser.add_argument("--session-id", help="Opaque CoKernel Session ID")
    return parser


def main() -> int:
    args = build_parser().parse_args()
    if args.version:
        print(__version__)
        return 0
    if not args.socket or not args.session_id:
        raise SystemExit("--socket and --session-id are required")

    connect_and_run(args.socket, args.session_id)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

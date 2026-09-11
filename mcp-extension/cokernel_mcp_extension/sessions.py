"""Resolve the already-running Jupyter kernel for a notebook path."""

from __future__ import annotations

import asyncio
import json
import posixpath
from collections.abc import Iterable, Mapping
from typing import Any
from urllib.parse import unquote
from urllib.request import Request, urlopen


class SessionResolutionError(RuntimeError):
    """Raised when CoKernel cannot unambiguously attach to an existing session."""


def normalize_notebook_path(value: str) -> str:
    """Normalize a Jupyter API path without touching the filesystem."""
    path = unquote(value or "").replace("\\", "/").lstrip("/")
    normalized = posixpath.normpath(path)
    return "" if normalized == "." else normalized


def _session_path(session: Mapping[str, Any]) -> str:
    direct = session.get("path")
    if isinstance(direct, str):
        return normalize_notebook_path(direct)

    # Older Jupyter session models exposed the notebook path in a nested object.
    notebook = session.get("notebook")
    if isinstance(notebook, Mapping):
        nested = notebook.get("path")
        if isinstance(nested, str):
            return normalize_notebook_path(nested)
    return ""


def _session_kernel_id(session: Mapping[str, Any]) -> str | None:
    kernel = session.get("kernel")
    if not isinstance(kernel, Mapping):
        return None
    kernel_id = kernel.get("id")
    return kernel_id if isinstance(kernel_id, str) and kernel_id else None


def select_existing_kernel_id(
    sessions: Iterable[Mapping[str, Any]], notebook_path: str
) -> str:
    """Return the unique running kernel ID for notebook_path or fail closed."""
    target = normalize_notebook_path(notebook_path)
    if not target:
        raise SessionResolutionError("notebook_path must not be empty in connect mode")

    kernel_ids: list[str] = []
    for session in sessions:
        if _session_path(session) != target:
            continue
        kernel_id = _session_kernel_id(session)
        if kernel_id and kernel_id not in kernel_ids:
            kernel_ids.append(kernel_id)

    if not kernel_ids:
        raise SessionResolutionError(
            f"No running Jupyter session exists for '{target}'. "
            "Open that notebook in JupyterLab and start its kernel, then retry. "
            "CoKernel refuses to create a second kernel in connect mode."
        )

    if len(kernel_ids) > 1:
        joined = ", ".join(kernel_ids)
        raise SessionResolutionError(
            f"More than one running kernel exists for '{target}' ({joined}). "
            "Close duplicate notebook sessions or pass an explicit kernel_id. "
            "CoKernel will not guess between kernels."
        )

    return kernel_ids[0]


def fetch_jupyter_sessions(
    jupyter_url: str, jupyter_token: str, *, timeout: float = 5.0
) -> list[Mapping[str, Any]]:
    """Fetch the Jupyter session list using the server token."""
    if not jupyter_url:
        raise SessionResolutionError("JUPYTER_URL is required for same-kernel attachment")
    if not jupyter_token:
        raise SessionResolutionError("JUPYTER_TOKEN is required for same-kernel attachment")

    endpoint = f"{jupyter_url.rstrip('/')}/api/sessions"
    request = Request(
        endpoint,
        headers={
            "Accept": "application/json",
            "Authorization": f"token {jupyter_token}",
        },
        method="GET",
    )

    try:
        with urlopen(request, timeout=timeout) as response:  # noqa: S310 - configured local Jupyter URL
            payload = json.load(response)
    except Exception as exc:  # keep credentials out of the error surface
        raise SessionResolutionError(
            "Could not query Jupyter sessions. Verify JUPYTER_URL, JUPYTER_TOKEN, "
            "and that the Jupyter service is healthy."
        ) from exc

    if not isinstance(payload, list):
        raise SessionResolutionError("Jupyter /api/sessions returned an unexpected payload")
    return payload


def resolve_existing_kernel_id_sync(
    jupyter_url: str, jupyter_token: str, notebook_path: str
) -> str:
    sessions = fetch_jupyter_sessions(jupyter_url, jupyter_token)
    return select_existing_kernel_id(sessions, notebook_path)


async def resolve_existing_kernel_id(
    jupyter_url: str, jupyter_token: str, notebook_path: str
) -> str:
    """Async wrapper that keeps blocking urllib work off the MCP event loop."""
    return await asyncio.to_thread(
        resolve_existing_kernel_id_sync,
        jupyter_url,
        jupyter_token,
        notebook_path,
    )

"""Jupyter MCP extension enforcing CoKernel's same-kernel invariant."""

from __future__ import annotations

import os
from typing import Any

from jupyter_mcp_server.extensions import JupyterMCPExtension
from reactor import PluginCompatibility, PluginManifest

from .sessions import resolve_existing_kernel_id


class CoKernelSessionAttachExtension(JupyterMCPExtension):
    """Wrap use_notebook so connect mode attaches to the browser's kernel."""

    def manifest(self) -> PluginManifest:
        return PluginManifest(
            name="cokernel-session-attach",
            version="0.1.0",
            description="Attach MCP notebook connections to an existing Jupyter session kernel.",
            author="CoKernel",
            compatibility=PluginCompatibility(api_version="v1"),
        )

    def register_tools(self, mcp: Any) -> None:
        manager = getattr(mcp, "_tool_manager", None)
        if manager is None:
            raise RuntimeError("Jupyter MCP tool manager is unavailable")

        try:
            original = manager._tools["use_notebook"].fn
        except (AttributeError, KeyError) as exc:
            raise RuntimeError(
                "Upstream use_notebook tool was not found; CoKernel cannot enforce same-kernel attachment"
            ) from exc

        manager.remove_tool("use_notebook")

        @mcp.tool()
        async def use_notebook(
            notebook_name: str,
            notebook_path: str,
            mode: str = "connect",
            kernel_id: str | None = None,
        ):
            """Use a notebook while preserving CoKernel's one-notebook/one-kernel invariant.

            In connect mode, omitting kernel_id attaches to the kernel already serving the
            notebook in JupyterLab. If there is no unique running session, the call fails
            instead of silently creating a second kernel. Passing kernel_id explicitly is
            an intentional override. Create mode retains upstream behavior.
            """
            resolved_kernel_id = kernel_id
            if mode == "connect" and not resolved_kernel_id:
                resolved_kernel_id = await resolve_existing_kernel_id(
                    os.environ.get("JUPYTER_URL", ""),
                    os.environ.get("JUPYTER_TOKEN", ""),
                    notebook_path,
                )

            return await original(
                notebook_name=notebook_name,
                notebook_path=notebook_path,
                mode=mode,
                kernel_id=resolved_kernel_id,
            )

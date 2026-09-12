"""Jupyter MCP extension enforcing CoKernel runtime invariants."""

from __future__ import annotations

import os
from typing import Any

from jupyter_mcp_server.extensions import JupyterMCPExtension
from mcp.server.transport_security import TransportSecuritySettings
from reactor import PluginCompatibility, PluginManifest

from .sessions import resolve_existing_kernel_id


_COKERNEL_ALLOWED_MCP_HOSTS = [
    "127.0.0.1:*",
    "localhost:*",
    "[::1]:*",
    # The OpenAI tunnel-client reaches the MCP service over the private
    # Compose network as http://mcp:4040/mcp, so its HTTP Host header is
    # mcp:4040. Keep DNS-rebinding protection enabled and admit only that
    # internal service name in addition to the SDK's localhost defaults.
    "mcp",
    "mcp:*",
]

_COKERNEL_ALLOWED_MCP_ORIGINS = [
    "http://127.0.0.1:*",
    "http://localhost:*",
    "http://[::1]:*",
]


def configure_cokernel_transport_security(mcp: Any) -> None:
    """Allow the private Compose tunnel client without disabling host checks.

    Jupyter MCP Server delegates Streamable HTTP host validation to the MCP
    Python SDK. Its safe default only admits localhost Host headers. CoKernel's
    tunnel-client is a separate container and addresses this service by its
    Compose DNS name (``mcp:4040``), so an otherwise healthy tunnel receives
    HTTP 421 during ``initialize``.

    Wrap app construction before Jupyter MCP starts uvicorn and supply an
    explicit allowlist. This retains DNS-rebinding protection for Windows/WSL
    loopback access while adding only CoKernel's private Compose service name.
    """

    if getattr(mcp, "_cokernel_transport_security_configured", False):
        return

    original_streamable_http_app = getattr(mcp, "streamable_http_app", None)
    if original_streamable_http_app is None:
        raise RuntimeError("Jupyter MCP streamable HTTP application factory is unavailable")

    security = TransportSecuritySettings(
        enable_dns_rebinding_protection=True,
        allowed_hosts=list(_COKERNEL_ALLOWED_MCP_HOSTS),
        allowed_origins=list(_COKERNEL_ALLOWED_MCP_ORIGINS),
    )

    def streamable_http_app(*args: Any, **kwargs: Any):
        kwargs.setdefault("transport_security", security)
        return original_streamable_http_app(*args, **kwargs)

    mcp.streamable_http_app = streamable_http_app
    mcp._cokernel_transport_security_configured = True


class CoKernelSessionAttachExtension(JupyterMCPExtension):
    """Enforce CoKernel's network and same-kernel runtime invariants."""

    def manifest(self) -> PluginManifest:
        return PluginManifest(
            name="cokernel-session-attach",
            version="0.1.1",
            description=(
                "Attach MCP notebook connections to an existing Jupyter session kernel "
                "and configure CoKernel transport security."
            ),
            author="CoKernel",
            compatibility=PluginCompatibility(api_version="v1"),
        )

    def register_tools(self, mcp: Any) -> None:
        configure_cokernel_transport_security(mcp)

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

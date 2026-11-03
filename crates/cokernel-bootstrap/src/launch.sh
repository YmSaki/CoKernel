#!/bin/sh
set -eu
/opt/cokernel/bin/cokernel-bootstrap check-boundary
cd /var/lib/cokernel/state
exec /usr/bin/env -i \
 PATH=/opt/cokernel/bin:/opt/cokernel/python/bin:/usr/lib/wsl/lib:/usr/bin:/bin \
 HOME=/var/lib/cokernel/state LC_ALL=C.UTF-8 \
 UV_CACHE_DIR=/var/cache/cokernel/uv UV_PYTHON_INSTALL_DIR=/var/cache/cokernel/python \
 COKERNEL_WORKLOAD_UID=61002 COKERNEL_WORKLOAD_GID=61002 \
 COKERNEL_PROJECTS_ROOT=/var/lib/cokernel/projects \
 COKERNEL_PROJECT_REGISTRY=/var/lib/cokernel/state/projects.json \
 COKERNEL_WORKER_PACKAGE=/opt/cokernel/worker \
 COKERNEL_SESSION_SOCKET_DIR=/var/lib/cokernel/sessions \
 COKERNEL_FAILURE_DIR=/var/lib/cokernel/state/failures \
 COKERNEL_RUNTIME_SOCKET=/var/lib/cokernel/state/runtime.sock \
 COKERNEL_MCP_EXECUTABLE=/opt/cokernel/bin/cokernel-mcp \
 COKERNEL_MCP_BEARER_FILE=/var/lib/cokernel/state/bearer \
 COKERNEL_MCP_PORT=0 \
 COKERNEL_TUNNEL_EXECUTABLE=/opt/cokernel/bin/tunnel-client \
 COKERNEL_TUNNEL_STATE_DIR=/var/lib/cokernel/state \
 /opt/cokernel/bin/cokernel-runtime "$@"

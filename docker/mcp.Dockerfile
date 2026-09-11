FROM ghcr.io/astral-sh/uv:0.12.12 AS uv

FROM python:3.12-slim-bookworm

COPY --from=uv /uv /uvx /usr/local/bin/

ENV PYTHONDONTWRITEBYTECODE=1 \
    PYTHONUNBUFFERED=1 \
    UV_LINK_MODE=copy

WORKDIR /opt/cokernel-mcp-extension
COPY mcp-extension/ ./

RUN uv pip install --system . \
    && useradd --uid 10001 --create-home --shell /usr/sbin/nologin cokernel-mcp

USER cokernel-mcp

EXPOSE 4040
ENTRYPOINT ["jupyter-mcp-server"]

FROM ghcr.io/astral-sh/uv:0.12.12 AS uv

FROM python:3.12-slim-bookworm

COPY --from=uv /uv /uvx /usr/local/bin/

ENV DEBIAN_FRONTEND=noninteractive \
    PYTHONDONTWRITEBYTECODE=1 \
    PYTHONUNBUFFERED=1 \
    UV_LINK_MODE=copy \
    PATH=/opt/cokernel/bin:$PATH

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates \
        curl \
        git \
        procps \
        tini \
    && rm -rf /var/lib/apt/lists/*

RUN uv venv /opt/cokernel \
    && uv pip install --python /opt/cokernel/bin/python \
        "jupyterlab>=4.4,<5" \
        "jupyter-collaboration>=4,<5" \
        "jupyter-mcp-tools>=0.1.4" \
        "ipykernel>=6.30,<7"

RUN groupadd --gid 1000 cokernel \
    && useradd --uid 1000 --gid 1000 --create-home --shell /bin/bash cokernel \
    && mkdir -p /workspace \
    && chown -R cokernel:cokernel /workspace /home/cokernel

COPY --chown=root:root scripts/container-entrypoint.sh /usr/local/bin/cokernel-entrypoint
RUN chmod 0755 /usr/local/bin/cokernel-entrypoint

USER cokernel
WORKDIR /workspace

EXPOSE 8888

ENTRYPOINT ["/usr/bin/tini", "--", "/usr/local/bin/cokernel-entrypoint"]

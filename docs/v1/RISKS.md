# CoKernel v1 Risk Register

Status: **implementation baseline**

| ID | Risk | Likelihood | Impact | Mitigation / gate |
|---|---|---:|---:|---|
| RK-001 | uv overlay/tooling launch does not give the worker the intended Project imports/interpreter semantics across supported Python versions | Medium | High | Phase-0 spike; use uv-supported project + `--with` overlay behavior; integration tests before Session implementation |
| RK-002 | IPython embedding/output capture misses common notebook semantics or breaks rich display | Medium | High | Phase-0 prototype against fixture corpus; explicitly scope supported MIME/magic/async behavior; preserve raw evidence for unsupported cases |
| RK-003 | Native/CUDA code can write directly to stdout/stderr or crash process | High | Medium | structured protocol on private Unix socket; capture process stdout/stderr separately; child-process failure containment |
| RK-004 | Python worker interrupt cannot stop some native calls | High | Medium | distinguish Interrupt from Restart; escalation path terminates process tree; never pretend interrupt succeeded |
| RK-005 | Multiple Human/AI execution requests corrupt shared Session order | Medium | High | single FIFO queue per Session; operation IDs; concurrency tests |
| RK-006 | Parallel Sessions exhaust RAM/VRAM and cause system instability | High | High | metrics/resource warnings; clear failure evidence; future admission controls; real stress tests; no claim of unlimited concurrency |
| RK-007 | `.ipynb` round-trip loses unknown metadata/outputs | Medium | High | typed core + opaque JSON preservation; fixture suite; atomic write; compatibility tests |
| RK-008 | Concurrent editor/output/external file writes overwrite changes | Medium | High | one Notebook Document Service; revision numbers; expected-revision mutations; external-change detection |
| RK-009 | Windows->WSL import path reintroduces broad Windows filesystem exposure | Low | High | controlled byte streaming through Host bridge; no permanent automount; path containment tests |
| RK-010 | Long-lived WSL bridge fails to keep distro alive reliably on real Windows | Medium | High | early real-machine Host/bridge acceptance; preserve v0.1 keeper knowledge as fallback/reference |
| RK-011 | Host/Runtime partial-version mismatch produces undefined behavior | Medium | High | handshake protocol/capabilities + version manifest; fail closed with repair/update instruction |
| RK-012 | Rust MCP ecosystem/SDK does not support required current Streamable HTTP annotations cleanly | Medium | Medium | Phase-0 SDK/protocol spike; keep MCP behind adapter crate; protocol can be implemented directly or service language changed without domain changes |
| RK-013 | Tunnel client startup/reconnect repeats v0.1 race/failure modes | Medium | High | MCP readiness dependency; Tunnel Supervisor; bounded reconnect; reuse prior real-machine sequencing tests |
| RK-014 | Tunnel/external credential leaks into worker environment/logs | Low | Critical | Windows secret store; separate service identity; explicit secret-taint/redaction tests; inspect worker argv/env in acceptance |
| RK-015 | WSL filesystem/user permissions block Project or shared uv cache operations | Medium | Medium | installer user/group design test; all paths on WSL-native filesystem; component tests under actual workload identity |
| RK-016 | Tauri/WebView notebook editor becomes a large IDE project and delays core runtime | Medium | Medium | implement core CLI/runtime first; UI scope limited to usable cells/packages/sessions; no IDE-parity goal |
| RK-017 | Notebook output volume freezes UI/MCP or exhausts memory | High | High | per-event/per-operation limits; stream/chunking; explicit truncation; stress tests |
| RK-018 | Session crash root-cause detection overclaims OOM/CUDA causes | Medium | Medium | evidence-based FailureRecord with confidence/classification; unknown remains unknown |
| RK-019 | Package environment changes during live Session create confusing mixed state | High | Medium | environment generation + `STALE_ENVIRONMENT`; no automatic restart/hot reload; explicit UI |
| RK-020 | uv cache cleanup invalidates assumptions about environment storage | Medium | Medium | use documented uv behavior; treat env as reconstructible; avoid unsupported symlink-store coupling; repair/sync path |
| RK-021 | JAX/PyTorch/CUDA compatibility varies by driver/package versions | High | Medium | release compatibility matrix; uv error visibility; representative GPU smoke tests; do not hard-code unsupported universal claims |
| RK-022 | Legacy reset deletes wanted notebook data | Low | Critical | explicit detection/warning/export option/confirmation; destructive test only on disposable legacy runtime |
| RK-023 | Update terminates Sessions and loses important live state | Medium | High | update impact analysis; active-Session gate; defer/confirm; no hidden maintenance restart |
| RK-024 | Worker can access product/tunnel secrets through filesystem permissions | Low | Critical | Linux identities and restrictive state/socket permissions; security acceptance running as actual worker user |
| RK-025 | MCP tool marked safer than real behavior and client makes wrong policy decision | Medium | High | annotation snapshot tests; execute-cell remains open-world/side-effecting; safe inspection separately constrained |

## Risk handling discipline

- High/Critical-impact risks require explicit tests/gates, not only comments.
- Phase-0 spikes exist specifically to retire RK-001, RK-002, RK-010 assumptions early and reduce RK-012 uncertainty.
- A risk discovered during implementation is added here when it can affect architecture, security, data loss, or release acceptance.
- `TEST_ACCEPTANCE.md` is the verification source; this register explains why those gates exist.
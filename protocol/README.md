# CoKernel protocol fixtures

This directory contains language-neutral fixtures for protocol version 1.

Authoritative semantics are documented in `docs/v1/INTERFACE_CONTRACTS.md` and `docs/v1/DOMAIN_MODEL.md`. Rust structs and the Python worker protocol tests must round-trip these fixtures.

Rules:

- UTF-8 JSON payloads;
- Host/Runtime and Runtime/Worker streams use a 32-bit big-endian frame length before each JSON payload;
- IDs are opaque strings (UUID in normal implementation);
- timestamps are RFC 3339 UTC;
- unknown optional fields should be tolerated when safe;
- frame/message size limits are enforced before allocating unbounded payloads.

Initial fixtures:

- `fixtures/bridge-hello.request.json`
- `fixtures/runtime-status.response.json`
- `fixtures/session-execute.request.json`
- `fixtures/session-output.event.json`
- `fixtures/error.response.json`

These fixtures are intentionally examples, not a replacement for typed validation and bounds checking.
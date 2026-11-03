# Actual remote connection prerequisites — 2026-09-22

Targeted upstream-contract audit of the working-tree Phase 7 connection path.
Authority: REQUIREMENTS workload/Windows boundaries -> ARCHITECTURE and
SECURITY_OBSERVABILITY sections 2–7 -> INSTALL_UPDATE section 5 and connection
contracts. IMPLEMENTATION_ORDER/WORK_STATUS schedule the work but cannot waive
the boundaries. This is a prerequisite audit, not a new complete security audit.

## Findings

1. **Remote acceptance needs the managed filesystem boundary.** The Phase 7
   different-UID fixture proves Linux file/proc/socket denial and lifecycle with
   synthetic keys. It uses the development Ubuntu distro, which is not evidence
   that Windows drives and executable interop are disabled. A workload that can
   traverse a Windows drive can reach unrelated Windows user data even when its
   Linux UID differs from the Runtime UID. Real keys must not be introduced to
   this development fixture merely because its UID tests passed.
2. **The sequential schedule omitted this prerequisite.** Phase 7 requires real
   remote access, while fresh managed distro provisioning is scheduled after the
   Phase 8 Desktop. The earlier UID audit resolved launch identity but left
   filesystem/interop validation to Phase 9. That is sufficient for the synthetic
   primitive tests, not the real-account connection consumer. The earliest
   incomplete artifact is the provisioning dependency in IMPLEMENTATION_ORDER;
   the product's security requirement itself is coherent.
3. **Credential input is not secure-runtime readiness.** The native human prompt
   and Host configuration owner can safely store a key without proving that the
   target WSL instance is ready for its handoff. Before using a real key, validation
   must cover the actual Host launch target, managed identities, permissions,
   mounts/interop and the worker's observed restrictions. A filename, configured
   flag, local MCP READY or tunnel CONNECTED cannot stand in for that proof.

## Resolution and next executable work

D-028 moves the minimal dedicated connection-validation environment prerequisite
into Phase 7. Implement its reusable provisioning/validation primitive in the
bootstrap component; Phase 9 later drives it from the full installer. Do not add
a second runtime or modify the user's development/legacy distro to make a test
pass. Use an owned unique validation identity, checked rootfs/payload, fixed WSL
policy and recorded Host target. A collision must fail without overwrite or
unregister. Validate with synthetic credentials before actual account connection.

Keep all Phase 9 install/reboot/legacy-reset and Phase 11 acceptance rows open.
The actual provider account/tunnel selection also remains required. This audit
does not infer that access is available or authorize copying keys from unrelated
credential stores. P7-06's recorded local primitive criterion stays valid; the
parent remote/installed-system gates stay open.

# Roadmap

## Phase 1: Protocol Research

Status: complete for AetherSDR source inspection.

- PGXL direct TCP path documented.
- TGXL direct TCP path documented.
- Manual IP MVP path confirmed.

## Phase 2: Validated Mock MVP

Status: complete for direct socket connection and polling.

- Mock bridge compiles.
- Local TCP smoke test passes.
- Real AetherSDR direct PGXL/TGXL connection and stable polling confirmed.
- Protocol transcript capture added for correction loop.

## Phase 3: Elecraft Hardware Validation

- KPA500 read-only command mapping mostly verified on COM21.
- KAT500 read-only command mapping verified on COM8.
- Capture serial transcripts.
- Replace placeholder parsers with validated parsers.
- Soak-test serial polling and reconnect behavior.

## Phase 3A: Operational Stability

Status: in progress.

- Soak-test command added.
- Serial reconnect backoff and stale-state transitions added.
- `/status` expanded with runtime counters.
- Windows power/USB guidance added.
- RF-risk operate remains design-only.

## Phase 4: AetherSDR Applet Activation

- Direct socket bridge: continue PGXL/TGXL protocol correction and real hardware validation.
- TGXL applet: validate Oliver's installed binary against the `0.9.5` direct-presence fix, then reset applet layout if the binary is current.
- PGXL applet: requires radio-side amplifier presence or an AetherSDR change.
- Flex API proxy: minimal amplifier-status injection design exists, keep separate and disabled unless radio-side configuration or an AetherSDR patch cannot solve applet presence.

## Phase 5: Service And Packaging

- Windows service installer.
- Release artifacts.
- Rotating logs.
- Support bundle export.

## Phase 6: Security Hardening

- Authenticated local control API.
- Deployment guidance for tunnels/reverse proxies.
- Optional TLS strategy where compatible.
- Rate limits and allowlists.

## Phase 7: Broader Client Compatibility

- SmartSDR for macOS validation.
- SmartSDR Windows investigation.
- Maestro compatibility investigation.

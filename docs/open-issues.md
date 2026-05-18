# Open Issues

## Must Fix Before WAN Exposure

- Raw PGXL/TGXL ports have no authentication.
- No TLS on raw protocol ports.
- Token config is reserved but not enforced for raw PGXL/TGXL sessions.
- No IP allowlist.
- No rate limiting for repeated malformed packets.
- No tested tunnel/reverse-proxy deployment profile.

## Must Fix Before Public Release

- Validate against current AetherSDR builds on macOS.
- Resolve PGXL applet activation path: AetherSDR requires radio-side `amplifier` presence for AMP visibility in the inspected source.
- Resolve TGXL binary/source mismatch: inspected AetherSDR source supports direct TGXL presence, but the tested binary reportedly did not show the TUN applet.
- Validate corrected KPA500/KAT500 command mappings against hardware transcripts.
- Expand Elecraft parsers only from official references or captured hardware responses.
- Add installer or service deployment path.
- Add rotating file logs.
- Add support bundle export containing config redaction, logs, and transcripts.
- Validate serial reconnect and stale-state recovery with physical USB disconnect/reconnect tests.

## Protocol Unknowns

- Exact real PGXL `info` response body.
- Exact real TGXL `info` response body.
- Whether richer direct `firmware` and `capabilities` fields affect AetherSDR UI behaviour.
- Whether PGXL applet visibility should be solved by native Flex radio configuration, an AetherSDR patch, a Flex API proxy, or another documented SmartSDR-compatible path.
- Whether the user's AetherSDR build includes the `TunerModel::m_directPresence` fallback for TGXL.
- Whether AetherSDR tolerates extra `connection_state` and `fault` fields in status responses.
- Exact PGXL direct TCP command set for operate/standby, if any.
- Exact error codes used by real PGXL/TGXL devices.
- Real TGXL autotune lifecycle timing and messages.
- Whether AetherSDR requires additional fields during longer sessions.

## Elecraft Command Unknowns

- KPA500 active read-only mapping is hardware-validated except `^SN;`, which still needs transcript evidence.
- KAT500 active read-only mapping is hardware-validated on COM8.
- KAT500 read-only commands beyond `RV;`, `SN;`, `AN;`, `BYP;`, `MD;`, `TP;`, `FLT;`, `VSWR;`, and `VFWD;` are not implemented.
- Real serial response variants need captures from Oliver's firmware versions.
- Serial transcript capture is implemented but needs real KPA500/KAT500 validation.
- KAT500 manual relay move command is classified as `destructive_or_unknown` until official mapping is found.
- Band/frequency tracking commands need validation.

## Performance And Scaling

- Multi-client command conflict policy is not defined.
- Metrics endpoint is simple JSON over localhost and not optimized for frequent scraping.
- Protocol transcript writing is per-line and should be reviewed under long sessions.
- Serial and protocol transcript rotation exist, but long soak sessions still need disk-growth validation.
- Soak-test memory/runtime stats are limited to bridge counters and do not yet include process RSS/handle counts.

## SmartSDR Compatibility

- Validate whether SmartSDR for macOS uses the same direct PGXL/TGXL behaviour.
- Capture any differences in polling, handshake, or required fields.
- Investigate SmartSDR Windows and Maestro expectations.

## Applet Activation

- Determine whether Flex can be configured to advertise synthetic PGXL/TGXL amplifier records without real Genius hardware.
- Validate candidate injected `amplifier` status records against AetherSDR in mock mode.
- Verify Oliver's installed macOS AetherSDR binary is `0.9.5` or newer, or otherwise contains the direct TGXL presence fix from #2250.
- Run the AetherSDR layout reset helper after confirming the binary contains direct TGXL presence.
- Keep Flex API proxy mode separate from direct PGXL/TGXL emulation.
- Decide whether to propose an AetherSDR patch for PGXL direct-presence fallback analogous to TGXL.

## Security/TLS/Auth

- Decide whether raw PGXL/TGXL emulation can be authenticated without breaking clients.
- Add TLS or document a supported reverse-proxy/tunnel pattern.
- Implement token-authenticated management/metrics access if exposed beyond loopback.

## Windows Service

- Add Windows service install/uninstall.
- Add service recovery policy.
- Define service account and log directory permissions.

## Installer/Packaging

- Build signed Windows release artifacts.
- Provide default config installation path.
- Add upgrade/rollback procedure.

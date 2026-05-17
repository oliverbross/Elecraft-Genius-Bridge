# Open Issues

## Protocol Unknowns

- Exact real PGXL `info` response body.
- Exact real TGXL `info` response body.
- Exact PGXL direct TCP command set for operate/standby, if any.
- Exact error codes used by real PGXL/TGXL devices.
- Real TGXL autotune lifecycle timing and messages.
- Whether AetherSDR requires additional fields during longer sessions.

## Elecraft Command Unknowns

- KPA500 command mapping is placeholder and unverified.
- KAT500 command mapping is placeholder and unverified.
- Real serial status response formats need official docs or captures.
- Serial transcript capture is implemented but needs real KPA500/KAT500 validation.
- KAT500 manual relay move command is classified as `destructive_or_unknown` until official mapping is found.
- Band/frequency tracking commands need validation.

## Security Hardening

- Raw PGXL/TGXL ports have no auth in MVP.
- Token config is reserved but not used for raw protocol.
- Need rate limits and malformed-packet counters.
- Need IP allowlists or deployment guidance for tunnels/reverse proxies.

## Windows Service

- Add Windows service install/uninstall.
- Add rotating file logs.
- Add service recovery policy.
- Add support bundle export.

## SmartSDR For Mac Compatibility

- Validate whether SmartSDR for macOS uses the same direct PGXL/TGXL behavior.
- Capture any differences in polling, handshake, or required fields.

## SmartLink/WAN Deployment

- Document tested tunnel options.
- Avoid recommending unsafe public port forwarding.
- Investigate whether AetherSDR peripheral manual IP works cleanly across each WAN path.

# PGXL Fast Connect Analysis

Latest evidence showed TGXL direct TCP connected immediately while PGXL direct TCP connected about 39.7 seconds later. The PGXL listener was already running, KPA/KAT polling was healthy, and Flex accepted `amplifier create`, so the delay is not a socket-listener failure.

## Timing Points To Capture

Evidence bundles now include `pgxl-delayed-connect-analysis.md`, which records:

- TGXL first connect timestamp.
- PGXL first connect timestamp.
- PGXL minus TGXL delta.
- PGXL/TGXL listener ready lines.
- First `amplifier create`.
- First `meter create`.
- First `interlock create`.
- First `keepalive enable`.
- First `sub amplifier all`.
- Whether registration continued without an amplifier handle.

## Suspected Cause

The last working AetherSDR path depended on repeated radio-side amplifier presence queries after `amplifier create`. Phase 56 restored registration continuation after Flex accepts the create command but does not broadcast a handle. Phase 57 adds a bounded startup burst:

1. Send `amplifier create`.
2. Wait briefly for an amplifier status/handle.
3. Continue with meters, interlock, keepalive, and subscriptions if the create response was accepted.
4. Send `sub amplifier all` once quickly after post-registration, then once per second for up to 10 seconds or until PGXL TCP connects.
5. Suppress the normal periodic amplifier/tuner refresh while the startup burst is active, preventing duplicate `sub amplifier all` bursts.
6. Return to the configured `amplifier_reannounce_interval_ms` cadence.

This keeps the known-good AetherSDR profile intact and does not recreate the amplifier object, churn handles, or send the rejected `amplifier set operate=1` connect-assist command.

## Connection Attempt Detection

EGB logs:

- `PGXL listener started ...` in `listener-startup.log`
- `PGXL accept peer=...` in `listener-startup.log`
- PGXL session start and disconnect events in the protocol logs
- every `sub amplifier all` and amplifier reannounce in `flex-tx.log`

Rust's normal `TcpListener` API sees accepted sockets, not raw TCP SYN packets. If `PGXL accept` is absent, EGB did not receive an accepted TCP connection; use Windows `netstat -ano`, Packet Monitor, or Wireshark on port `9008` to prove whether AetherSDR attempted a SYN before the delayed accept.

## Success Target

PGXL direct TCP should start within 5 seconds of TGXL/direct listener readiness and without repeated manual AetherSDR connect clicks.

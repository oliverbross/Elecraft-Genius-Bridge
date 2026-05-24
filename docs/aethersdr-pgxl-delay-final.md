# AetherSDR PGXL Delay Final

Phase 63 evidence confirms the PGXL delayed connection is not caused by EGB listener readiness or PGXL protocol failure.

Known facts:

- TGXL connects immediately.
- PGXL connects later, typically about `33-40s` after TGXL.
- Example latest run:
  - TGXL first connect: `1779542002363`
  - PGXL first connect: `1779542035642`
  - Delta: `33.279s`
- `pgxl-self-probe` connects to EGB PGXL immediately and receives valid `info` / `status`.
- Once AetherSDR opens PGXL TCP, the session is stable.
- Latest PGXL session: hundreds of status commands, `0` parse failures.
- `amplifier_removed_count=0`.

Conclusion: EGB is ready before AetherSDR opens TCP. Phase 65 found the missing
piece: in the latest evidence the PGXL listener was bound to `127.0.0.1:9008`
but the Flex amplifier status advertised `192.168.0.189`. AetherSDR's automatic
PGXL path uses the advertised Flex amplifier IP, so the immediate auto-open was
pointed at an address where this EGB instance was not listening. The later
successful PGXL session came from AetherSDR's local/manual peripheral path, which
used `127.0.0.1`.

The inspected AetherSDR source appears intended to call `connectToPgxl(ampIp)` immediately from `RadioModel::amplifierChanged(true)` when `ampIp` is non-empty. If the installed binary still waits tens of seconds, the next practical fix is an AetherSDR-side patch or logging run.

## Patch Recommendation

Minimal AetherSDR patch direction:

- In the `amplifierChanged(true)` handler, log `ampHandle`, `ampIp`, current PGXL connection state, and timestamp.
- If `ampIp` is non-empty and PGXL direct is not connected, call `connectToPgxl(ampIp)` immediately.
- Avoid waiting for any delayed retry timer when the radio-side amplifier object already contains a reachable IP.

EGB should not add destructive or fake lifecycle hacks for this delay. The fix is
address consistency:

- same-host AetherSDR: bind PGXL to `127.0.0.1` and advertise `127.0.0.1`;
- LAN AetherSDR: bind PGXL to the Windows LAN IP and advertise that LAN IP.

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

Conclusion: EGB is ready before AetherSDR opens TCP. The remaining delay is AetherSDR-side direct-PGXL open timing, gating, or retry behaviour.

The inspected AetherSDR source appears intended to call `connectToPgxl(ampIp)` immediately from `RadioModel::amplifierChanged(true)` when `ampIp` is non-empty. If the installed binary still waits tens of seconds, the next practical fix is an AetherSDR-side patch or logging run.

## Patch Recommendation

Minimal AetherSDR patch direction:

- In the `amplifierChanged(true)` handler, log `ampHandle`, `ampIp`, current PGXL connection state, and timestamp.
- If `ampIp` is non-empty and PGXL direct is not connected, call `connectToPgxl(ampIp)` immediately.
- Avoid waiting for any delayed retry timer when the radio-side amplifier object already contains a reachable IP.

EGB should not add destructive or fake lifecycle hacks for this delay. The EGB PGXL server is already available and stable.

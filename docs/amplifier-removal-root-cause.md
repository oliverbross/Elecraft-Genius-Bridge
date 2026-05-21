# Amplifier Removal Root Cause

## Current Evidence

Recent live evidence showed Flex status lines reporting the injected amplifier object as removed, followed by Flex API degradation and PGXL TCP aborts.

Phase 45 adds explicit detection for this condition:

- `flex_injection.amplifier_removed_count`
- `flex_injection.last_amplifier_removed_reason`
- `flex_diagnostics.amplifier_removed_count`
- `flex_diagnostics.last_amplifier_removed_reason`
- `disconnect-events.jsonl` event: `flex_amplifier_removed`

## Most Likely Causes To Validate

1. Duplicate or churned `amplifier create` lifecycle.
   EGB should create one amplifier object per Flex API session and keep that handle stable until the session reconnects.

2. Advertised IP not reachable by the client/radio path.
   A real PGXL advertises a LAN-reachable IP and port. Avoid `127.0.0.1` when the client runs on macOS or another host.

3. Flex-side operate hack rejection.
   `pgxl_connect_assist` can send `amplifier set <handle> operate=1`. This has been observed to return `500000A7` in some runs and should not be treated as the lifecycle foundation.

4. Interlock or antenna-map mismatch.
   The PGXL API creates an AMP interlock with valid antennas. A mismatch with the radio TX antenna may cause SmartSDR interlock problems or object churn.

5. Missing keepalive/ping or stale pending commands.
   The official API says keepalive feeds the radio watchdog. Evidence must confirm `keepalive enable` is accepted and pings receive responses.

## Phase 45 Corrections

- TGXL direct status now follows the official `S<seq>|status ...` model.
- Flex radio context is subscribed and used for TGXL `freqA`, `bandA`, `modeA`, and `flexA`.
- Amplifier removal is now counted and exported instead of being treated as an ordinary amplifier status.
- PGXL/TGXL evidence includes protocol/status sample files for replay and comparison.

## Validation Needed

Run a live AetherSDR test with the operational profile and confirm:

- `amplifier_removed_count` remains `0`.
- `amplifier_handle_change_count` remains `0` or `1` for a single Flex API session.
- `PGXL_STABLE` persists for 10 minutes.
- A band change updates `/status.radio_context` and TGXL `freqA`/`bandA`.
- Repeated Tune presses execute one KAT500 `T;` each without stale dry-run blocks.

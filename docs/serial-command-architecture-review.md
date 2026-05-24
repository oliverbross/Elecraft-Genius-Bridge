# Serial Command Architecture Review

Phase 65 reviewed the serial side for deterministic operation under live
polling, frequency follow, band follow, and Tune commands.

## KPA500

Implemented:

- Expected-prefix response matching for KPA500 queries.
- Live state from `^OS`.
- Power/SWR from `^WS`.
- Temperature from `^TM`.
- PA supply voltage/current from `^VI`.
- Fault from `^FL`.
- Safe standby command `^OS0;`.
- RF-risk operate command `^OS1;` only when explicitly enabled.
- Experimental band follow via `^BNnn;`.

State propagation:

- KPA telemetry changes update shared state immediately.
- KPA state changes request a bounded Flex amplifier reannounce burst.
- PGXL direct status is derived from the live shared KPA state, not cached
  defaults.

Remaining limitations:

- KPA500 has a verified band command, not a verified direct frequency command.
- `^BNnn;` confirmation can be delayed by stale serial responses; EGB now tracks
  requested/confirmed band and stale counts.

## KAT500

Implemented:

- Shared command queue for polling and control commands.
- Frequency-follow command `F <kHz>;`.
- Exact confirmation tracking for `F <kHz>;`.
- Tune command `T;` with `TP;` polling until complete.
- SWR refresh after Tune.
- Stale/unmatched response classification.

Production rules:

- Control commands must use the same serial ownership path as polling.
- Frequency-follow requests are coalesced and must not run concurrently.
- `tuning=true` must be cleared by fresh `TP0` or by timeout expiry.
- Stale `F` echoes must never be reported as confirmed frequency matches.

## Concurrency Review

The serial architecture is acceptable for production use under these conditions:

- Polling and control share the single-owner command path.
- Follow commands pause or serialize normal polling.
- Confirmation is based on exact expected response, not the next line received.
- Unsolicited lines are logged and routed, not assigned to the wrong command.

## Remaining Risk

High stale-response counts can still occur during rapid Flex band changes. This
is now handled as an efficiency/diagnostic issue rather than a correctness
failure: EGB reports exact confirmation separately from stale responses and does
not mark stale data as confirmed.

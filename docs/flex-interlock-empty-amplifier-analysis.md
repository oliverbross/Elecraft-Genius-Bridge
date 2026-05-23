# Flex Interlock Empty Amplifier Analysis

Observed Flex status:

```text
interlock tx_client_handle=0x00000000 state=READY reason=AMP:PG-XL source= tx_allowed=1 amplifier=
```

The empty `amplifier=` field is suspicious because the reason names the PGXL interlock, but current evidence does not prove it is fatal when `tx_allowed=1`.

## Current Classification

- `reason=AMP:PG-XL`, `amplifier=`, `tx_allowed=1`: WARN only.
- `reason=AMP:PG-XL`, `tx_allowed=0`: `INTERLOCK_BLOCKED` and degraded reason.
- Non-empty `amplifier=`: clears the empty-amplifier warning.

The bridge now exposes:

- `last_interlock_state`
- `last_interlock_reason`
- `last_interlock_tx_allowed`
- `empty_amplifier_field_count`
- `interlock_blocked_count`

## Open Questions

- Whether Flex ever populates `amplifier=` for external PGXL API clients in this lifecycle.
- Whether AetherSDR or SmartSDR treats empty `amplifier=` as an applet/control blocker when `tx_allowed=1`.
- Whether `KPA500 STANDBY` contributes to `TX_FAULT` / `NOT_READY` transitions in SmartSDR.

## Validation

The next live run should inspect `flex-rx.log`, `flex-injection-health.md`, and `/status.flex_diagnostics`. Empty `amplifier=` is not a failure by itself unless the same status reports `tx_allowed=0`.

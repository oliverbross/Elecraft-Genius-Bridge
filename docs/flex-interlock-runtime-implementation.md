# Flex Interlock Runtime Implementation

## Implemented Behaviour

EGB now has an optional runtime AMP interlock loop controlled by:

```yaml
flex_injection:
  enable_runtime_interlock: true
```

When enabled, EGB watches Flex interlock status lines. If the radio reports
`state=PTT_REQUESTED` for the virtual AMP interlock, EGB evaluates the current
hardware/safety state and sends one of:

```text
interlock ready <interlock_id>
interlock not_ready <interlock_id>
```

This follows the SmartSDR TCP/IP interlock sequence where an Ethernet AMP
interlock must respond after PTT is requested.

## Safety Gate

EGB sends `ready` only when all conditions are true:

- runtime interlock is enabled,
- the interlock handle is known,
- RF-risk permission is enabled in the effective KPA policy,
- KPA500 polling is connected,
- KPA500 state is `OPERATE`,
- KPA500 fault state is clear.

Otherwise EGB sends `not_ready` and records the reason.

## Evidence

Each request/decision is written to:

```text
interlock-runtime-events.jsonl
```

`/status.flex_diagnostics` also exposes:

- `runtime_interlock_enabled`
- `interlock_runtime_event_count`
- `last_interlock_runtime_action`
- `last_interlock_runtime_result`
- `last_interlock_runtime_at_ms`

## Current Limitation

The runtime loop is disabled by default. It is enabled in the SmartSDR PGXL meter
test profile for observation, but without RF-risk permission it will answer
`not_ready` to transmit requests. That is intentional until RF-risk operation is
explicitly tested.

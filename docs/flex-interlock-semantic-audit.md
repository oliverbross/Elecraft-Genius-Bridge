# Flex Interlock Semantic Audit

## Sources

- FlexRadio PGXL Amplifier-to-Radio API, March 2024.
- SmartSDR TCP/IP interlock API: <https://github-wiki-see.page/m/flexradio/smartsdr-api-docs/wiki/TCPIP-interlock>
- Latest EGB evidence: `20260524-020425-evidence-test.zip`.

## What The Protocol Requires

The PGXL document lists the PGXL radio-side registration command as:

```text
interlock create type=AMP valid_antennas=ANT1,ANT2 name=PG-XL serial=<serial>
```

The general SmartSDR interlock API is stricter about the runtime state machine. Ethernet AMP interlocks start not-ready, emit `PTT_REQUESTED` when transmit is requested, and the external device is expected to send `interlock ready <id>` before RF is allowed. After PTT release, AMP interlocks return to not-ready.

That means interlock creation alone is not proof that the virtual amplifier is operational. The readiness path is:

1. EGB creates an AMP interlock.
2. Flex emits an interlock status with state and `tx_allowed`.
3. On `state=PTT_REQUESTED`, EGB must decide whether the KPA500 is ready.
4. If ready, EGB should send `interlock ready <id>`.
5. If not ready, EGB should leave the interlock not-ready or explicitly report a blocked condition.

## Current EGB Behaviour

Current EGB creates the PGXL-style interlock and records the returned handle. It tracks:

- `last_interlock_state`
- `last_interlock_reason`
- `last_interlock_tx_allowed`
- `interlock_blocked_count`
- empty `amplifier=` warnings

Phase 69 adds explicit transition instrumentation:

- `last_interlock_transition`
- `last_interlock_transition_at_ms`
- `last_tx_allowed_transition`
- `last_tx_allowed_transition_at_ms`
- `amplifier_operable_eligibility`
- `external_control_capable_state`

## Gap

EGB does not yet implement the full dynamic interlock ready/not-ready exchange. It creates the interlock, but it does not respond to `PTT_REQUESTED` with `interlock ready <id>`. This can explain SmartSDR transmit/interlock failures and why the Flex-side amplifier may be treated as only partially operable.

It does not, by itself, explain AetherSDR AMP button silence. AetherSDR source sends AMP clicks as Flex commands when the button path fires, and prior interlock-disabled runs still produced no `amplifier set` command.

## Interpretation Of Empty `amplifier=`

Latest evidence showed `reason=AMP:PG-XL` with an empty `amplifier=` field in some interlock statuses. The public interlock docs do not require an `amplifier=` field in the status examples. Therefore:

- `tx_allowed=1` with empty `amplifier=` is WARN.
- `tx_allowed=0` with `reason=AMP:PG-XL` is BLOCKED.
- Missing interlock status entirely leaves amplifier operability unknown.

## Required Production Fix

Implement a real interlock runtime loop:

1. Parse `PTT_REQUESTED`.
2. Check KPA500 state, fault, band, and safety policy.
3. Send `interlock ready <id>` only when safe.
4. Track response/latency.
5. Send or allow not-ready when KPA500 is unavailable/faulted or RF-risk policy blocks operation.

Until that is implemented, EGB should not claim SmartSDR transmit/interlock compatibility.

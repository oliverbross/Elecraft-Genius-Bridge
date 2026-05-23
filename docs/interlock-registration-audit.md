# Interlock Registration Audit

Status: Phase 54.

## Official Syntax

The PGXL Amplifier-to-Radio API documents this interlock create shape:

```text
interlock create type=AMP valid_antennas=ANT1,ANT2 name=PG-XL serial=<pgxl-serial>
```

Documented fields:

- `type`: interlock type, shown as `AMP` in the PGXL example.
- `valid_antennas`: comma-separated radio antenna ports.
- `name`: interlock identifier.
- `serial`: serial number associated with the interlock.

The official document does not show an explicit `amplifier=<handle>` parameter for `interlock create`.

## Previous EGB Command

```text
interlock create type=AMP valid_antennas=ANT1,ANT2 name=PG-XL serial=EGB-KPA500
```

This command matched the documented fields, but EGB sent it in the same command burst as `amplifier create` and the meter creates. Latest evidence showed the radio later emitted:

```text
interlock tx_client_handle=0x00000000 state=READY reason=AMP:PG-XL source= tx_allowed=1 amplifier=
```

The empty `amplifier=` field suggests the radio had not associated the interlock with the amplifier object.

## Phase 54 Change

EGB now uses a two-stage Flex registration sequence:

1. Send `amplifier create`.
2. Wait for the radio to accept and/or broadcast the amplifier handle.
3. Only then send meter creates, interlock create, keepalive, and subscriptions.

The interlock create line itself is unchanged because the public PGXL API does not document an amplifier handle parameter.

## Readiness Rule

If Flex emits an interlock status with `reason=AMP:PG-XL` and an empty `amplifier=` field, EGB records:

- `last_interlock_status_line`
- `interlock_amplifier_field_empty=true`
- `interlock_empty_amplifier_count`

Operational readiness is marked FAIL until the association is correct or the remaining hidden requirement is identified.

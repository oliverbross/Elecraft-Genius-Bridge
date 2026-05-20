# PGXL Regression Diff

## Symptom

AetherSDR sees the Flex amplifier object and shows the AMP pane, but does not open TCP 9008. This means the direct PGXL server is not yet in the path; the failure is in the Flex amplifier pairing/advertisement fields that precede the socket connection.

## Working-Era Behaviour

The working direct-connect profile advertised the amplifier with PGXL-style direct-connect fields on the `amplifier create` command:

```text
amplifier create ip=<egb-ip> port=9008 model=PowerGeniusXL serial_num=<serial> ant=ANT1:PORTA,ANT2:PORTB state=<live> connected=1 configured=1 enabled=1 direct=1 lan=1
```

Earlier code accidentally used a hard-coded `state=STANDBY`. That helped expose the pairing fields but broke live state correctness.

## Broken-Era Behaviour

Phase 29 removed `state=STANDBY` from direct-connect create profiles to avoid stale state. That also removed a likely AetherSDR trigger field from the pre-socket amplifier advertisement.

## Current Fix

Direct-connect profiles now restore create-time `state`, but derive it from live KPA500 shared state:

- `^OS1;` -> `state=OPERATE`
- `^OS0;` -> `state=STANDBY`
- real KPA fault -> `state=FAULT`

`strict_real_pgxl` stays conservative for protocol audits and may not trigger AetherSDR TCP 9008.

## Recommended Profile

Use:

```yaml
flex_injection:
  amplifier_status_profile: aethersdr_force_direct
  trace_amplifier_advertisements: true
```

Then run:

```powershell
.\target-msvc\debug\egb.exe compare-pgxl-profiles --config .\config.aethersdr-known-good.yaml --duration-seconds 60
```

Inspect:

- `amplifier-advertisements.jsonl`
- `pgxl-regression-diff.md`
- `pgxl-connect-attempt-timeline.md`

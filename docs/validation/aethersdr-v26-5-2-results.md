# AetherSDR v26.5.2 Results

Status: Phase 19 notes from Oliver's validation.

## Observed

- TUN pane appears.
- AMP pane appears.
- TGXL direct endpoint on TCP `9010` shows connected.
- PGXL direct endpoint on TCP `9008` does not show connected in AetherSDR Peripherals.
- PGXL direct status replies still report `state=STANDBY` when KPA500 reports `^OS0;`.

## Interpretation

The AMP pane is being created through Flex radio-side amplifier presence. Direct PGXL socket compatibility is sufficient for background polling but is not yet satisfying the Peripherals connected indicator.

## Phase 19 Changes To Validate

- Full PGXL radio-side registration sequence is now sent.
- `ant_map` default is now `ANT1:PORTA,ANT2:PORTB`.
- KPA500 response matching now waits for the expected prefix, avoiding stale response assignment.
- KPA500 `^WS030 011;` remains parsed as 30 W and SWR 1.1.

## Capture Checklist

- Screenshot AetherSDR Peripherals before and after Phase 19.
- Capture PGXL protocol transcript around `C1|info` and `status`.
- Capture Flex registration logs and radio responses.
- Record whether PGXL changes from `Not connected` to connected.

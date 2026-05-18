# PGXL SmartSDR Gap Analysis

Status: Phase 19 working note.

## Current Observations

- AetherSDR shows both TUN and AMP panes.
- TGXL direct/manual endpoint on TCP `9010` reports connected.
- PGXL direct/manual endpoint on TCP `9008` accepts connections and stable polling, but AetherSDR Peripherals still reports PGXL `Not connected`.
- SmartSDR sees the injected tuner.
- SmartSDR does not yet see the injected amplifier when EGB only sends `amplifier create`.
- Flex radio status logs show interlock updates with `amplifier=` empty.

## Reference Contract

The FlexRadio PGXL Amplifier-to-Radio API describes a broader registration sequence:

```text
amplifier create ip=<ip> port=9008 model=PowerGeniusXL serial_num=<serial> ant=ANT1:PORTA,ANT2:PORTB
meter create name=FWD type=AMP min=30.0 max=63.01 units=DBM
meter create name=RL type=AMP min=34.0 max=60.0 units=DB
meter create name=DRV type=AMP min=10.0 max=50.00 units=DBM
meter create name=ID type=AMP min=0.0 max=70.0 units=AMPS
meter create name=TEMP type=AMP min=0.0 max=100.0 units=TEMP_C
interlock create type=AMP valid_antennas=ANT1,ANT2 name=PG-XL serial=<serial>
keepalive enable
ping
```

Phase 19 now sends this sequence when:

```yaml
flex_injection:
  full_pgxl_registration: true
  create_meters: true
  create_interlock: true
```

## Expected Improvement

The radio should now have the same object categories a real PGXL registers:

- Amplifier object with `model=PowerGeniusXL`.
- AMP meter objects for forward power, return loss, drive, current, and temperature.
- AMP interlock bound to the same serial number.
- Keepalive watchdog fed by periodic ping.

This should give SmartSDR a better chance of showing the amplifier and should help AetherSDR associate the AMP pane with a real radio-side amplifier object.

## Remaining Blocker

Live Flex meter value publication is not implemented. The PGXL API document points to the SmartSDR metering protocol, but the exact external-amplifier value update path still needs capture evidence before implementation. Until then, live values come from the direct PGXL socket backed by KPA500 polling.

## Next Capture

Run with `logging.level: debug` and capture:

- `FLEX TX >` registration lines.
- All `R<seq>|...` responses for amplifier, meters, interlock, keepalive, and subscription.
- Any `S...|amplifier ...` status line.
- Any `S...|meter ...` or interlock status line.

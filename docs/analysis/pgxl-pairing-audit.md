# PGXL Pairing Emulation Audit

## Sources

- AetherSDR local source: `research/AetherSDR`, commit `6d17b3bbda96b836762e7d40758a1fc3e14725f9`
- FlexRadio PGXL Amplifier-to-Radio API documentation: https://www.flexradio.com/documentation/power-genius-xl-api-documentation/
- FlexRadio SmartSDR amplifier API examples discussed in the Flex community: `amplifier create ip=<ip> port=9008 model=PowerGeniusXL serial_num=<serial> ant=ANT1:PORTA,ANT2:PORTB`

## AetherSDR Trigger

AetherSDR automatic PGXL direct TCP is triggered by `RadioModel::amplifierChanged(true)` in `src/gui/MainWindow.cpp`.

The inspected source opens direct PGXL only when:

- a radio-side amplifier status has a non-empty `model` that is not `TunerGeniusXL`
- `RadioModel::ampIp()` is non-empty
- `PgxlConnection` is not already connected

The auto-connect path uses only the `ip` value:

```cpp
m_pgxlConn.connectToPgxl(m_radioModel.ampIp());
```

The inspected source does not read `port` from the radio-side amplifier status for auto-connect. It defaults to PGXL port `9008`.

Manual PGXL connection is independent and uses saved Peripherals settings:

- `PGXL_ManualIp`
- `PGXL_ManualPort`

## Documented PGXL Registration Fields

The conservative real-PGXL registration command is:

```text
amplifier create ip=<ip> port=9008 model=PowerGeniusXL serial_num=<serial> ant=ANT1:PORTA,ANT2:PORTB
```

Known companion objects from the PGXL API:

- `meter create name=FWD type=AMP min=30.0 max=63.01 units=DBM`
- `meter create name=RL type=AMP min=34.0 max=60.0 units=DB`
- `meter create name=DRV type=AMP min=10.0 max=50.00 units=DBM`
- `meter create name=ID type=AMP min=0.0 max=70.0 units=AMPS`
- `meter create name=TEMP type=AMP min=0.0 max=100.0 units=TEMP_C`
- `interlock create type=AMP valid_antennas=ANT1,ANT2 name=PG-XL serial=<serial>`
- `keepalive enable`
- `sub amplifier all`

## EGB Profiles

`strict_real_pgxl` emits the conservative field set only:

```text
amplifier <handle> model=PowerGeniusXL ip=<ip> port=9008 serial_num=<serial> ant=ANT1:PORTA,ANT2:PORTB state=<state>
```

`pgxl_paired` uses the documented `amplifier create` command and records live KPA500-derived status candidates.

`pgxl_verbose`, `old_good_pgxl`, and `aethersdr_force_direct` remain AetherSDR-oriented profiles. They add fields such as `connected`, `configured`, `enabled`, `direct`, and `lan`. These are not confirmed real-PGXL fields and should not be treated as production behavior. All profiles now advertise live KPA500 state; none should force `state=STANDBY` outside mock state.

## Current Suspects

If AetherSDR shows AMP but never opens TCP 9008:

- the IP advertised in the Flex amplifier status may be loopback or otherwise unreachable from macOS
- the AetherSDR binary may not receive the amplifier status that EGB sees through the radio API
- the radio may create an amplifier object but not rebroadcast an `ip=...` field to GUI clients
- manual PGXL settings may be empty or stale
- the inspected source may differ from the installed binary

The new pairing diagnostics expose:

- last amplifier status line
- pairing candidate fields
- whether any PGXL TCP session started after amplifier status
- last amplifier reannounce reason

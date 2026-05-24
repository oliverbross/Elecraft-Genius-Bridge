# Flex Meter Publication Deep Review

## Sources

- FlexRadio PGXL Amplifier-to-Radio API, March 2024.
- SmartSDR TCP/IP meter API: <https://github-wiki-see.page/m/flexradio/smartsdr-api-docs/wiki/TCPIP-meter>
- AetherSDR local source: `src/core/PanadapterStream.cpp`, `src/models/MeterModel.*`.

## What The Protocol Supports

The SmartSDR meter API explicitly supports externally defined meters. `meter create` returns:

```text
<meter_number>,<stream_id>
```

After that, the radio accepts meter values through UDP VITA-49 meter packets on port 4991. The packet must use:

- the returned stream identifier,
- the returned meter identifier,
- the correct VITA meter packet class/format,
- unit-specific scaling.

The PGXL document also points PGXL meter values at the Flex metering protocol.

## Current EGB Behaviour

EGB creates the PGXL meter objects:

- FWD
- RL
- DRV
- ID
- TEMP

EGB stores meter handles, but it does not store/parse the returned stream IDs separately and does not send VITA-49 UDP meter packets. Therefore:

- `meter_publish_supported=false` in `/status` means "not implemented in EGB", not "impossible in the Flex API".
- `meter_publish_count=0` is expected.
- SmartSDR can see a PGXL amplifier object but cannot receive live PGXL meter values through EGB yet.

## Does Meter Publication Gate Operate?

Evidence is inconclusive. AetherSDR source does not appear to require meter values before emitting the AMP button command; its direct click path sends `amplifier set <handle> operate=<0|1>` when the applet click fires and `m_ampHandle` is non-empty. SmartSDR may use meter availability for display quality, but meter publication alone is not proven to be the missing operate-command trigger.

## Feasibility

Meter publication is feasible but requires a dedicated VITA-49 UDP transmitter:

1. Preserve both meter number and stream ID from each `meter create` response.
2. Build VITA meter packets for FWD/RL/DRV/ID/TEMP.
3. Send packets to the radio's standard VITA-49 UDP port.
4. Use documented scaling:
   - DB/DBM/DBFS: `value * 128`
   - VOLTS/AMPS: `value * 256`
   - TEMPC/TEMPF: `value * 64`
5. Rate-limit updates and keep them synchronized with KPA500 telemetry.

## Production Recommendation

Do not claim SmartSDR PGXL live-data compatibility until VITA-49 meter publication is implemented and verified. This is the largest remaining SmartSDR PGXL data-path gap.

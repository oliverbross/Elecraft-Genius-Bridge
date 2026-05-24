# Flex VITA-49 Meter Publisher

## Implemented Scaffold

EGB now has an optional VITA-49 AMP meter publisher controlled by:

```yaml
flex_injection:
  enable_vita_meter_publish: true
```

When enabled and Flex returns meter IDs/stream IDs from `meter create`, EGB sends
VITA-49 meter extension packets to the radio UDP port `4991`.

The packet follows the documented SmartSDR meter requirements:

- stream identifier from the `meter create` response,
- meter identifier from the `meter create` response,
- packet class code `0x8002`,
- payload as big-endian `(meter_id, raw_i16_value)` pairs.

## Telemetry Mapping

| Flex meter | Source | Scaling |
| --- | --- | --- |
| `FWD` | KPA500 forward power converted to dBm | `value * 128` |
| `RL` | KPA500 SWR converted to return loss dB | `value * 128` |
| `DRV` | Unknown drive power, currently `-120 dBm` | `value * 128` |
| `ID` | KPA500 PA current | `value * 256` |
| `TEMP` | KPA500 temperature C | `value * 64` |

## Status Fields

`/status.flex_diagnostics` reports:

- `meter_publish_supported`
- `meter_publish_count`
- `meter_publish_last_result`
- `last_meter_publish_ms`
- `last_meter_values`
- `meter_availability`

## Verification Needed

The implementation is intentionally gated behind `enable_vita_meter_publish`.
The packet format is based on the SmartSDR meter API and AetherSDR's local VITA
meter decoder, but it still needs live SmartSDR verification.

Use:

```powershell
.\target\release\egb.exe flex-runtime-test --config .\config.smartsdr-pgxl-meter-test.yaml --duration-minutes 5
```

Then verify whether SmartSDR PGXL meter values populate and whether
`meter_publish_count` increases.

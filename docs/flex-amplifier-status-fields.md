# Flex Amplifier Status Fields

The strict official profile sends the documented PGXL registration command:

```text
amplifier create ip=<egb-ip> port=9008 model=PowerGeniusXL serial_num=<serial> ant=ANT1:PORTA,ANT2:PORTB
```

AetherSDR auto-opens PGXL direct TCP when it receives a radio-side amplifier status with:

```text
amplifier <handle> model=PowerGeniusXL ip=<egb-ip>
```

The `ip` field is mandatory for auto-connect in the inspected source because `MainWindow` calls:

```cpp
m_pgxlConn.connectToPgxl(m_radioModel.ampIp());
```

## Status Profiles

`flex_injection.amplifier_status_profile` controls how aggressive EGB is during PGXL trigger experiments:

| Profile | Fields | Status |
| --- | --- | --- |
| `minimal` | `ip`, `port`, `model`, `serial_num`, `ant` | documented create fields |
| `pgxl_paired` | same create fields; paired synthetic evidence line logged | default experiment |
| `pgxl_verbose` | adds `state`, `connected`, `configured`, `enabled` | experimental |
| `old_good_pgxl` | same direct-connect readiness fields as verbose, without strict-real filtering | AetherSDR regression checks |
| `aethersdr_minimal` | adds live `state` only | recommended AetherSDR operational profile |
| `aethersdr_operational` | alias for `aethersdr_minimal` | compatibility with prior config names |
| `aethersdr_force_direct` | adds `direct`, `lan` to verbose fields | lab/regression only |

Only `minimal`/`official_pgxl`/`pgxl_paired` should be considered conservative. `aethersdr_minimal` is allowed for AetherSDR live use because the strict official create did not trigger AetherSDR direct TCP in latest evidence and Flex echoed back `state` while stripping the older readiness fields. The old force/lab profiles remain isolated diagnostics tools.

## Reannounce Behaviour

EGB does not create duplicate amplifier objects. The amplifier reannounce loop sends a rate-limited:

```text
sub amplifier all
```

and writes the latest synthetic amplifier line into:

```text
amplifier-status-lines.log
amplifier-reannounce.log
```

This is observability and refresh pressure, not a true Flex status injection. If the radio does not replay the amplifier status with `ip=<egb-ip>` to AetherSDR, a future Flex API proxy remains the clean way to inject exact client-visible status.

## Meter Publication

Meter objects are created for PGXL compatibility, but live external meter-value publication is still deferred. The current evidence does not prove a supported Flex API command that lets an external amplifier client publish arbitrary FWD/RL/DRV/ID/TEMP values back into the radio meter stream. Until that is captured, EGB uses direct PGXL TCP status for live KPA500 telemetry.

## Keepalive / Ping

EGB sends `keepalive enable` during full PGXL registration and starts a Flex API `ping` loop after registration. `/status` exposes:

- `ping_count`: ping commands sent.
- `ping_ack_count`: successful `R<seq>|0|...` responses to ping.
- `ping_failures`: rejected or failed pings.
- `last_ping_latency_ms`: time from ping send to response.

# Full PGXL/TGXL/Elecraft Bridge Map

Status: Phase 49 audit. This document is the operational map between Flex/AetherSDR/SmartSDR protocol surfaces and the real Elecraft KPA500/KAT500 hardware.

## Hard Readiness Rule

EGB must not advertise a healthy PGXL/Flex amplifier when KPA500 read-only preflight fails. A locked COM21 or failed KPA first poll now blocks PGXL/Flex amplifier startup with `KPA500_PORT_LOCKED_OR_UNAVAILABLE`.

EGB must not advertise loopback `127.0.0.1` as the PGXL IP when the Flex radio path is LAN. The advertised IP must be this Windows PC's reachable LAN IP unless the entire radio/client path is local-only.

## PGXL / Flex Amplifier To Elecraft KPA500

| Flex/AetherSDR surface | Direction | EGB source of data | Elecraft command | Status |
|---|---:|---|---|---|
| Flex `amplifier create ip=<egb-ip> port=9008 model=PowerGeniusXL serial_num=<serial> ant=...` | EGB -> Flex | Config plus KPA preflight gate | none | Implemented, blocked until KPA first poll succeeds |
| Flex `meter create FWD/RL/DRV/ID/TEMP` | EGB -> Flex | Config, KPA telemetry mapping | none | Implemented |
| Flex `interlock create type=AMP valid_antennas=...` | EGB -> Flex | Config antenna map | none | Implemented |
| Flex `keepalive enable` / ping | EGB -> Flex | Flex session lifecycle | none | Implemented, monitored |
| Flex `amplifier ... removed` | Flex -> EGB | Flex RX event | none | Operational readiness failure |
| PGXL TCP greeting `V...` | EGB -> AetherSDR | PGXL emulator | none | Implemented |
| PGXL `info` | AetherSDR -> EGB | Config/model identity | none | Implemented |
| PGXL `status` state | AetherSDR -> EGB | KPA `^OS;`, `^FL;` | `^OS;`, `^FL;` | Implemented; must not emit `UNKNOWN` after healthy poll |
| PGXL `status` forward power/SWR | AetherSDR -> EGB | KPA `^WS;` | `^WS;` | Implemented |
| PGXL `status` temperature | AetherSDR -> EGB | KPA `^TM;` | `^TM;` | Implemented |
| PGXL `status` current/voltage | AetherSDR -> EGB | KPA `^VI;` | `^VI;` | Implemented internally; PGXL `vac` remains conservative because KPA PA supply is not AC mains |
| PGXL/Flex standby request | Client -> EGB -> KPA | Effective control policy | `^OS0;`, verify `^OS;` | Implemented, safe-control gated |
| PGXL/Flex operate request | Client -> EGB -> KPA | Effective control policy | `^OS1;`, verify `^OS;` | Implemented, RF-risk gated |
| PGXL/Flex clear fault | Client -> EGB -> KPA | Advanced control policy | `^FLC;` | Blocked by default |

Runtime evidence fields:
- `/status.amp.first_poll_completed`
- `/status.amp.last_successful_command`
- `/status.pgxl_advertised_status`
- `/status.flex_injection.last_amplifier_status_line`
- `logs/serial/kpa500-serial.log`
- `diagnostics/runs/*/latest-kpa-telemetry.json`

## TGXL To Elecraft KAT500

| TGXL/AetherSDR surface | Direction | EGB source of data | Elecraft command | Status |
|---|---:|---|---|---|
| TGXL TCP greeting `V...` | EGB -> AetherSDR | TGXL emulator | none | Implemented |
| TGXL `info` | Client -> EGB | Config/model identity | none | Implemented |
| TGXL `status` `freqA/bandA/modeA/flexA` | Client -> EGB | Flex slice/tx radio context | none | Implemented |
| TGXL `status` `swr/fwd/antA/bypass/state/tuning` | Client -> EGB | KAT polling plus tune lifecycle | `VSWR;`, `VFWD;`, `AN;`, `BYP;`, `TP;`, `FLT;` | Implemented |
| TGXL `flexradio read/get/list` | Client -> EGB | Flex radio context/config | none | Implemented read-only/defaulted |
| TGXL `catradio read/get` | Client -> EGB | Flex radio context/config | none | Implemented read-only/defaulted |
| TGXL `setup read` / `ifconf read` | Client -> EGB | Config/defaults | none | Implemented read-only/defaulted |
| TGXL `autotune` | Client -> EGB -> KAT | Effective control policy plus Flex TX frequency | `F <kHz>;` when known, then `T;`, poll `TP;`/`VSWR;` | Implemented, KAT tune/RF-risk gated |
| TGXL `activate ant=N` | Client -> EGB -> KAT | Effective control policy | KAT antenna command if enabled/verified | Partial, gated |
| TGXL `bypass set=0/1` | Client -> EGB -> KAT | Effective control policy | KAT bypass command if enabled/verified | Partial, gated |
| TGXL `tune relay=<...>` | Client -> EGB | Unsupported mapping | none | Intentionally blocked until relay mapping is verified |
| TGXL config writes/save/bootloader | Client -> EGB | Safety policy | none | Blocked or no-op with stable response |

Runtime evidence fields:
- `/status.radio_context`
- `/status.tgxl_advertised_status`
- `/status.controls.last_tune_frequency_hz`
- `/status.controls.last_tune_result`
- `logs/serial/kat500-serial.log`
- `diagnostics/runs/*/tune-band-decision.md`
- `diagnostics/runs/*/kat500-tune-sequence.log`

## Operational Gates

Operational readiness is a failure if any of these are true:
- Runtime commit differs from repository HEAD when running from a development checkout.
- KPA500 preflight cannot open/poll the configured COM port while PGXL/Flex amplifier support is enabled.
- KAT500 preflight cannot open/poll the configured COM port while TGXL support is enabled.
- Flex amplifier advertised IP is loopback while the radio path is LAN.
- Flex reports `amplifier <handle> removed`.
- Amplifier create is repeated due to reconnect churn.
- `pgxl_connect_assist` is enabled in a normal operational/evidence run.
- The operational amplifier profile would add non-standard fields to `amplifier create`.

The operational create command must be exactly:

```text
amplifier create ip=192.168.0.189 port=9008 model=PowerGeniusXL serial_num=EGB-KPA500 ant=ANT1:PORTA,ANT2:PORTB
```

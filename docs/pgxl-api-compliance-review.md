# PGXL API Compliance Review

Authoritative source: FlexRadio / 4O3A Power Genius XL API and PGXL Amplifier-to-Radio API documentation.

## Direct PGXL TCP Audit

| Item | Expected behavior | EGB support | KPA500/Flex mapping | Safety | Action |
|---|---|---|---|---|---|
| TCP port | PGXL direct control on TCP 9008 | Full | PGXL emulator | Read-only | Implemented. |
| Greeting | server greeting/version line | Full for AetherSDR | none | Read-only | Existing `V0.1.0-egb-pgxl`. |
| Command frame | `C<seq>|command` | Full for AetherSDR | parser shared with TGXL | Read-only | Implemented. |
| Response frame | `R<seq>|code|body` | Full | formatter shared | Read-only | Implemented. |
| `info` | identity/version/model | Full enough for AetherSDR | static EGB PGXL identity | Read-only | Implemented. |
| `status` | live PGXL telemetry | Full field presence, partial real PGXL semantics | KPA500 `^OS;`, `^WS;`, `^TM;`, `^VI;`, `^FL;` | Read-only | Implemented from shared state. |
| `setup read` | read setup/config | Partial safe placeholder | none | Read-only | Added in Phase 47. |
| `ifconf read` | read network config | Partial safe placeholder | none | Read-only | Added in Phase 47. |
| `catradio read=A/B` | read CAT radio config | Partial safe placeholder | none | Read-only | Added in Phase 47. |
| `flexradio read=A/B` | read Flex pairing config | Partial from Flex context | Flex radio context | Read-only | Added in Phase 47. |
| config `set` / `save` | mutate PGXL config or reboot/apply | Intentionally unsupported | none | State-change/config | Stable error returned; EGB config is YAML/GUI-owned. |
| direct `operate` | request operate | Partial desired-state mapping | KPA500 `^OS1;` only through safety gates | RF-risk | Accepted as desired state; execution gated in KPA driver. |
| direct `standby` | request standby | Full desired-state mapping | KPA500 `^OS0;` | Safe control | Accepted and routed through safety gates. |

## Flex Amplifier Registration Lifecycle

| Step | Required sequence | EGB status | Notes |
|---|---|---|---|
| Wait for `H<handle>` | Do not send object create before radio assigns client handle | Full | Registration starts after handle. |
| `amplifier create ip=<ip> port=9008 model=PowerGeniusXL serial_num=<serial> ant=...` | Register external amplifier | Full | Uses configured LAN IP. Avoid `127.0.0.1` unless the client is local. |
| Meter create FWD | `meter create name=FWD type=AMP min=30.0 max=63.01 units=DBM` | Full create | Live value publication still evidence-gated. |
| Meter create RL | `meter create name=RL type=AMP min=34.0 max=60.0 units=DB` | Full create | Return-loss mapped from KPA SWR for status; meter push path pending. |
| Meter create DRV | `meter create name=DRV type=AMP min=10.0 max=50.00 units=DBM` | Full create | DRV value unknown. |
| Meter create ID | `meter create name=ID type=AMP min=0.0 max=70.0 units=AMPS` | Full create | KPA `^VI` current maps to current display where safe. |
| Meter create TEMP | `meter create name=TEMP type=AMP min=0.0 max=100.0 units=TEMP_C` | Full create | KPA `^TM` maps temperature. |
| Interlock create | `interlock create type=AMP valid_antennas=... name=PG-XL serial=<serial>` | Full create | Valid antenna map is configurable and must match radio TX antennas. |
| `keepalive enable` | Enable radio watchdog/keepalive | Full | Ping loop follows registration. |
| `sub amplifier all` | Subscribe amplifier status | Full | Duplicate subscription attempts are counted. |
| `sub slice all` | Subscribe frequency/slice context | Full | Source of TGXL freq/band. |
| `sub tx all` | Subscribe transmit antenna/context | Full | Source of TX antenna. |
| Periodic `ping` | Keep session alive | Full | `/status.flex_diagnostics` exposes ping counters. |
| Stable handle lifecycle | Preserve object until Flex session reconnect/removal | Full instrumentation, pending live proof | Phase 46 added lifecycle counters and removal timeline. |

## PGXL Status Mapping

| PGXL direct field | EGB source | Mapping |
|---|---|---|
| `state` | KPA500 `^OS;`, `^FL;` and shared amp state | `^OS1;` -> `OPERATE`, `^OS0;` -> `STANDBY`, fault only if fault code is non-zero. |
| `peakfwd` | KPA500 `^WSppp sss;` | forward watts converted to dBm, `-120` at no RF. |
| `swr` | KPA500 `^WSppp sss;` | SWR mapped to return-loss-style dB for AetherSDR compatibility. |
| `temp` | KPA500 `^TMnnn;` | Celsius. |
| `id` | KPA500 `^VIvvv iii;` current | current amps after KPA scaling. |
| `vac` | KPA500 `^VI` voltage | not published as VAC unless >=100V; KPA reports internal PA supply, not AC mains. |
| `meffa` | fault/MEFFA state | `OK` if no fault. |

## Flex Object Ownership

The radio owns the amplifier object once `amplifier create` is accepted. EGB owns:

- registration request,
- PGXL direct TCP server,
- KPA500 telemetry source,
- safety-gated desired control requests.

EGB must not fight the radio by repeatedly creating amplifier objects or constantly forcing operate state. Connect-assist remains a compatibility workaround and is not the lifecycle foundation.

## Current PGXL Compliance Estimate

PGXL/Flex compliance is estimated at **76%**:

- Full: 12 command/lifecycle groups.
- Partial: 5 groups.
- Missing: 2 groups.

Remaining blockers are live meter value publication, exact PGXL direct config semantics, and proving that Flex does not remove the amplifier object during a long live run.

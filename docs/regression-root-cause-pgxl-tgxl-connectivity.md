# PGXL/TGXL Connectivity Regression Root Cause

This document freezes the direct-client connectivity regression analysis. It intentionally does not propose new PGXL/TGXL protocol changes.

## Evidence Summary

| Evidence run | Commit | Config | Amplifier profile | PGXL sessions | TGXL sessions | Amplifier removed | Result |
| --- | --- | --- | --- | ---: | ---: | ---: | --- |
| `20260522-101428-evidence-test` | `f36d718` | `config.aethersdr-known-good.yaml` | `aethersdr_force_direct` | 1 | 1 | 0 | Last known good |
| `20260522-104521-evidence-test` | `0456ee3` | `config.aethersdr-real-operational.yaml` | `aethersdr_force_direct` | 0 | 0 | 5 | First observed bad |
| `20260522-110510-evidence-test` | `6c257ce` | `config.aethersdr-real-operational.yaml` | `official_pgxl` | 0 | 0 | 1 | Bad |
| `20260522-113417-evidence-test` | `a9e7d61` | `config.aethersdr-compatible-operational.yaml` | `aethersdr_operational` | 0 | 0 | 1 | Bad |
| `20260523-010259-evidence-test` | `18b3c67` | `config.aethersdr-compatible-operational.yaml` | `aethersdr_minimal` | 0 | 0 | 1 | Bad |
| `20260523-012702-evidence-test` | `fbae8ff` | `config.aethersdr-compatible-operational.yaml` | `aethersdr_minimal` | 0 | 0 | 0 | Flex stable, direct clients absent |

## Last Known Good

Last known good evidence:

```text
diagnostics/runs/20260522-101428-evidence-test
commit=f36d718
config=config.aethersdr-known-good.yaml
profile=aethersdr_force_direct
PGXL sessions started=1
TGXL sessions started=1
amplifier_removed_count=0
```

The relevant effective settings were:

```yaml
server.bind_ip: 127.0.0.1
tgxl.smartsdr_compat: true
tgxl.experimental_presence_refresh: true
flex_injection.amplifier_status_profile: aethersdr_force_direct
flex_injection.amplifier_reannounce_interval_ms: 5000
flex_injection.pgxl_connect_assist: false
flex_injection.amplifier_ip: 192.168.0.189
flex_injection.force_advertised_pgxl_ip: 192.168.0.189
kpa500.dry_run: true
kat500.dry_run: true
```

The observed Flex amplifier status seen by clients was:

```text
amplifier 0x0E3C1607 ip=192.168.0.189 model=PowerGeniusXL serial_num=EGB-KPA500 ant=ANT1:PORTA,ANT2:PORTB state=STANDBY
```

The emitted EGB compatibility status also included the old force-direct fields:

```text
state=<live> connected=1 configured=1 enabled=1 direct=1 lan=1
```

## First Observed Bad State

First observed bad evidence:

```text
diagnostics/runs/20260522-104521-evidence-test
commit=0456ee3
config=config.aethersdr-real-operational.yaml
profile=aethersdr_force_direct
PGXL sessions started=0
TGXL sessions started=0
amplifier_removed_count=5
```

The important config deltas versus the last-good run were:

- `server.bind_ip` changed from `127.0.0.1` to `192.168.0.189`.
- `tgxl.smartsdr_compat` changed from `true` to `false`.
- `tgxl.experimental_presence_refresh` changed from `true` to `false`.
- `flex_injection.amplifier_reannounce_interval_ms` changed from `5000` to `30000`.
- Real control mode was enabled, but that was not required for direct socket connection.

The strongest connectivity regression candidate is the loss of the last-good AetherSDR refresh cadence and profile combination, not the Elecraft serial layer. Later commits then moved the recommended AetherSDR configs to stricter or minimal profiles, which removed the exact `aethersdr_force_direct` path that had produced PGXL/TGXL `1/1`.

## Restore Decision

The locked regression config is:

```text
config.aethersdr-last-known-good-operational.yaml
```

It restores the last-good connection path exactly for regression testing:

- `aethersdr_force_direct`
- `tgxl.experimental_presence_refresh=true`
- `tgxl.smartsdr_compat=true`
- `amplifier_reannounce_interval_ms=5000`
- dry-run monitor controls
- KPA/KAT hardware polling enabled
- advertised PGXL IP `192.168.0.189`

This config is intentionally not a new protocol design. It is a rollback baseline so we can prove when PGXL/TGXL direct TCP comes back before resuming interlock/control work.

## Test Command

```powershell
.\target\release\egb.exe connection-regression-test --config .\config.aethersdr-last-known-good-operational.yaml --duration-minutes 5
```

PASS criteria:

- Flex API connected.
- Amplifier create accepted.
- `amplifier_removed_count=0`.
- `PGXL session started=true`.
- `TGXL session started=true`.
- Direct client commands received on both protocols.

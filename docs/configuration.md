# Configuration

See `config.example.yaml` for the authoritative example.

## Server

```yaml
server:
  bind_ip: 127.0.0.1
```

Default is loopback for safety. Use a LAN IP for AetherSDR on another machine. Avoid `0.0.0.0` unless you understand the security implications.

## Emulators

```yaml
pgxl:
  enabled: true
  port: 9008
  aethersdr_compat: false
  strict_emulation: false
  startup_delay_ms: 0

tgxl:
  enabled: true
  port: 9010
  aethersdr_compat: false
  strict_emulation: false
  startup_delay_ms: 0
  force_presence_test: false
```

Set `aethersdr_compat: true` while diagnosing AetherSDR direct PGXL/TGXL behaviour. Compatibility mode emits only source-observed fields, removes unverified `capabilities`, `firmware`, `connection_state`, and `fault` fields from protocol bodies, and reports `swr` as return loss dB because AetherSDR converts it back to an SWR ratio.

Set `strict_emulation: true` in mock mode to simulate a more realistic device startup sequence. The emulator sends the required `V` greeting immediately, but shared mock state reports transitional readiness until `startup_delay_ms` expires.

Set `tgxl.force_presence_test: true` only for AetherSDR TUN applet activation testing. It makes the direct TGXL emulator publish the richest safe direct state currently understood, without changing KAT500 serial behaviour.

## Elecraft Devices

```yaml
kpa500:
  enabled: true
  com_port: COM21
  baud: 38400
  polling_interval_ms: 1000
  mock: true
  dry_run: true
  allow_rf_risk: false

kat500:
  enabled: true
  com_port: COM8
  baud: 38400
  polling_interval_ms: 1000
  mock: true
  dry_run: true
  allow_rf_risk: false
```

Set `mock: false` only when real hardware is connected and command mappings have been checked for your firmware.

Set `dry_run: true` for first hardware tests. Dry-run opens the configured COM port and permits read-only status queries, but blocks control-changing commands such as operate, tune, antenna change, bypass, relay move, and clear fault.

Set `kpa500.allow_rf_risk: true` only for local controlled KPA500 operate testing after dummy-load / no-RF checks. It is required before EGB will translate a Flex/PGXL operate request to `^OS1;`. Standby still uses `^OS0;` and remains state-change-safe, but it is blocked when `dry_run: true`.

## Logging

```yaml
logging:
  level: info
  protocol_trace: false
  protocol_transcript_dir:
  serial_transcript_dir:
  transcript_rotate_bytes: 1048576
```

Use `debug` for internal diagnostic logs.

Set `protocol_trace: true` to print raw PGXL/TGXL lines with direction markers:

```text
PGXL RX < C1|status
PGXL TX > R1|0|...
TGXL RX < C1|status
TGXL TX > R1|0|...
```

Set `protocol_transcript_dir` to write one timestamped transcript file per client session.

Set `serial_transcript_dir` to write one timestamped KPA500/KAT500 serial transcript per device session. Transcript write failures are logged and then disabled for that session so polling is not blocked.

Set `transcript_rotate_bytes` to cap each serial or protocol transcript file. When the limit is reached, EGB opens the next indexed file for the same device/client session.

## Metrics

```yaml
metrics:
  enabled: true
  bind_ip: 127.0.0.1
  port: 9160
```

When enabled, `GET /status` returns localhost-only JSON with connection states, poll timestamps, firmware/capability fields, protocol counters, and client counts. The endpoint refuses non-loopback binds.

Phase 14 adds serial runtime counters to `/status`: poll successes/failures, reconnects, stale-state transitions, last/average/max poll latency, and stale duration.

## Flex Amplifier Injection

```yaml
flex_injection:
  enabled: false
  radio_ip: 127.0.0.1
  radio_port: 4992
  amplifier_ip: 127.0.0.1
  amplifier_port: 9008
  amplifier_model: PowerGeniusXL
  serial: EGB-KPA500
  handle: amp_1
  ant_map: ANT1:PORTA,ANT2:PORTB
  full_pgxl_registration: true
  create_meters: true
  create_interlock: true
  reconnect_initial_ms: 1000
  reconnect_max_ms: 30000
  ping_interval_ms: 30000
```

Phase 19 `flex_injection` is a LAN/local-only registration client. It connects to the Flex radio TCP API on port `4992` and sends the documented PGXL-style amplifier registration sequence so the radio can advertise a `PowerGeniusXL`-compatible amplifier to AetherSDR/SmartSDR.

`amplifier_ip` must be the Windows bridge IP that macOS AetherSDR can reach. AetherSDR should use that IP for its direct PGXL connection on `amplifier_port`.

The configured `handle` is an EGB log/config label. The real Flex amplifier object handle is assigned by the radio and observed by AetherSDR through normal radio status.

`full_pgxl_registration` enables the amplifier create, AMP meter create, AMP interlock create, keepalive, subscription, and periodic ping sequence. Disable `create_meters` or `create_interlock` only for protocol isolation tests.

This mode does not implement live radio-side meter value streaming, proxying, TLS, or WAN exposure. Operate remains RF-risk gated by `kpa500.allow_rf_risk`.

## Soak Test

Use soak mode for long-duration validation:

```powershell
cargo run -p egb -- soak-test --config config.hardware-readonly.yaml --duration-hours 4
```

It starts the normal bridge runtime and prints a health summary every 60 seconds. See `docs/soak-testing.md`.

## Mock Fault Simulation

```yaml
mock:
  pgxl_fault: false
  tgxl_fault: false
  high_swr: false
```

These flags only affect mock-mode state. Use them later for degraded UI testing, not for first applet activation tests.

## Control Verification

```yaml
control:
  verify_delay_ms: 200
```

No-ACK control commands use this delay before a follow-up verification query. KPA500 standby sends `^OS0;`, waits, then verifies with `^OS;` expecting `^OS0;`.

## Profiles

- `config.mock.yaml`: no hardware required, protocol trace enabled, strict startup simulation and TGXL direct-presence diagnostics enabled.
- `config.hardware-readonly.yaml`: COM8/COM21 hardware mode with `dry_run: true`.
- `config.aethersdr-compat-readonly.yaml`: COM8/COM21 hardware mode with `dry_run: true` and compatibility response formatting enabled for AetherSDR disconnect/app visibility testing.
- `config.flex-injection-readonly.yaml`: COM8/COM21 hardware mode with `dry_run: true`, AetherSDR compatibility formatting, and passive Flex amplifier registration enabled. Edit `flex_injection.radio_ip` and `flex_injection.amplifier_ip` before use.
- `config.hardware-control-local-only.yaml`: COM21 KPA500 control mode with `dry_run: false`, COM8 KAT500 still `dry_run: true`, loopback bind by default. Use only locally or on a private LAN after read-only validation.

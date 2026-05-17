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
  strict_emulation: false
  startup_delay_ms: 0

tgxl:
  enabled: true
  port: 9010
  strict_emulation: false
  startup_delay_ms: 0
  force_presence_test: false
```

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

kat500:
  enabled: true
  com_port: COM8
  baud: 38400
  polling_interval_ms: 1000
  mock: true
  dry_run: true
```

Set `mock: false` only when real hardware is connected and command mappings have been checked for your firmware.

Set `dry_run: true` for first hardware tests. Dry-run opens the configured COM port and permits read-only status queries, but blocks control-changing commands such as operate, tune, antenna change, bypass, relay move, and clear fault.

## Logging

```yaml
logging:
  level: info
  protocol_trace: false
  protocol_transcript_dir:
  serial_transcript_dir:
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

## Metrics

```yaml
metrics:
  enabled: true
  bind_ip: 127.0.0.1
  port: 9160
```

When enabled, `GET /status` returns localhost-only JSON with connection states, poll timestamps, firmware/capability fields, protocol counters, and client counts. The endpoint refuses non-loopback binds.

## Mock Fault Simulation

```yaml
mock:
  pgxl_fault: false
  tgxl_fault: false
  high_swr: false
```

These flags only affect mock-mode state. Use them later for degraded UI testing, not for first applet activation tests.

## Profiles

- `config.mock.yaml`: no hardware required, protocol trace enabled, strict startup simulation and TGXL direct-presence diagnostics enabled.
- `config.hardware-readonly.yaml`: COM8/COM21 hardware mode with `dry_run: true`.
- `config.hardware-control-local-only.yaml`: COM21 KPA500 control mode with `dry_run: false`, COM8 KAT500 still `dry_run: true`, loopback bind by default. Use only locally or on a private LAN after read-only validation.

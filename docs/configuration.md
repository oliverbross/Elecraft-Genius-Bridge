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

tgxl:
  enabled: true
  port: 9010
```

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

## Profiles

- `config.mock.yaml`: no hardware required, protocol trace enabled.
- `config.hardware-readonly.yaml`: COM8/COM21 hardware mode with `dry_run: true`.
- `config.hardware-control-local-only.yaml`: COM8/COM21 hardware mode with `dry_run: false`, loopback bind by default. Use only locally or on a private LAN after read-only validation.

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

kat500:
  enabled: true
  com_port: COM8
  baud: 38400
  polling_interval_ms: 1000
  mock: true
```

Set `mock: false` only when real hardware is connected and command mappings have been checked for your firmware.

## Logging

```yaml
logging:
  level: info
  protocol_trace: false
  protocol_transcript_dir:
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

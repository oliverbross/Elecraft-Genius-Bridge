# Packet Captures

No real PGXL or TGXL packet captures have been added yet.

Use this directory for validated captures and normalized transcripts:

```text
docs/packet-captures/
  pgxl/
    connect-handshake.txt
    status-idle.txt
    status-transmit.txt
  tgxl/
    connect-handshake.txt
    status-idle.txt
    status-operate.txt
    tune-cycle.txt
    relay-adjust.txt
    antenna-switch.txt
```

Capture-derived behavior should be promoted into `tools/protocol-replay/scenarios/` before production emulator implementation.


# Live Hardware Session

Use this runbook for the first real KPA500/KAT500 validation sessions.

## Safe Startup Order

1. Confirm the radio cannot accidentally transmit.
2. Connect KPA500 and KAT500 USB/serial cables.
3. Power on KAT500, then KPA500.
4. Leave the amplifier in standby.
5. Confirm Windows COM ports:

```powershell
cargo run -p egb -- list-serial
```

6. Start with the read-only profile:

```powershell
cargo run -p egb -- check-config --config config.hardware-readonly.yaml
cargo run -p egb -- test-kpa --config config.hardware-readonly.yaml
cargo run -p egb -- test-kat --config config.hardware-readonly.yaml
```

## Dummy Load And RF

- Use no RF for initial read-only serial validation.
- Use a dummy load before any tune or operate test.
- Start with the lowest practical drive level.
- Do not run unattended RF, tune, or operate tests.

## Transcript Collection

Enable:

```yaml
logging:
  level: debug
  protocol_trace: true
  protocol_transcript_dir: logs/protocol
  serial_transcript_dir: logs/serial
```

Keep:

- `logs/serial/kpa500-*.log`
- `logs/serial/kat500-*.log`
- `logs/protocol/pgxl-*.log`
- `logs/protocol/tgxl-*.log`
- screenshots from AetherSDR
- firmware versions shown by devices or serial responses

## First Successful Milestones

1. `egb list-serial` shows `COM21` and `COM8`, or the configured replacements.
2. `test-kpa` opens the KPA500 port and only sends read-only queries.
3. `test-kat` opens the KAT500 port and only sends read-only queries.
4. Serial transcripts show no operate, tune, antenna, bypass, relay move, or clear-fault command during read-only testing.
5. `/status` on localhost reports connection states and protocol counters:

```powershell
Invoke-WebRequest http://127.0.0.1:9160/status
```

6. AetherSDR connects in mock mode before any hardware-control run.
7. Hardware-control testing remains loopback or LAN-only, never WAN.

## Rollback And Recovery

1. Stop EGB with Ctrl+C.
2. Return to `config.hardware-readonly.yaml` or `config.mock.yaml`.
3. Put the KPA500 in standby from the front panel.
4. Stop RF drive at the radio.
5. Power-cycle hardware only if the device manual recommends it.
6. Preserve transcripts before restarting.

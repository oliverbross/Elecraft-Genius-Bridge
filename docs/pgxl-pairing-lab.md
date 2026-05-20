# PGXL Pairing Lab

Use this when AMP is visible but AetherSDR does not open a direct PGXL TCP session on port `9008`.

Run:

```powershell
.\target-msvc\debug\egb.exe pgxl-pairing-lab --config .\config.flex-injection-readonly.yaml --profile strict_real_pgxl --duration-minutes 5
```

During the run:

1. Connect AetherSDR to the radio.
2. Open the AMP pane.
3. In AetherSDR Peripherals, try PGXL manual connect if needed.
4. Leave the run active for the full duration.

The evidence bundle includes:

- `amplifier-status-lines.log`
- `amplifier-reannounce.log`
- `pgxl-pairing-analysis.md`
- `pgxl-protocol.log`
- `client-sessions.jsonl`
- `status-samples.jsonl`
- `summary.md`

Loopback-vs-LAN test:

```yaml
flex_injection:
  amplifier_ip: 127.0.0.1
  force_advertised_pgxl_ip: 192.168.1.50
```

Use the Windows bridge LAN IP for `force_advertised_pgxl_ip` when macOS AetherSDR is on another machine. If AetherSDR refuses or ignores `127.0.0.1`, this should cause TCP 9008 attempts to hit the Windows bridge.

Interpretation:

- `pgxl_session_started_count > 0`: AetherSDR attempted direct PGXL TCP.
- `pgxl_session_started_count = 0` and AMP visible: radio-side amplifier presence worked, but PGXL direct pairing trigger did not fire.
- `last_amplifier_status_line` lacks `ip`: AetherSDR cannot auto-connect from radio-side presence.
- `last_amplifier_status_line` has a loopback IP while AetherSDR is remote: advertise the Windows LAN IP instead.

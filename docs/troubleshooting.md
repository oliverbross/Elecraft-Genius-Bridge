# Troubleshooting

## AetherSDR Does Not Connect

- Confirm EGB is running.
- Confirm `server.bind_ip` is reachable from the AetherSDR machine.
- Confirm Windows Firewall allows inbound TCP on `9008` and `9010`.
- Confirm AetherSDR manual IP ports match the config.
- Set `logging.level: debug` and check for `PGXL RX` or `TGXL RX` log entries.

## Config Fails

Run:

```powershell
egb check-config --config config.yaml
```

Common issues:

- Invalid `server.bind_ip`.
- Empty COM port while `mock: false`.
- Polling interval set to zero.

## Serial Device Does Not Open

Run:

```powershell
egb list-serial
egb test-kpa --config config.yaml
egb test-kat --config config.yaml
```

Check that no other application owns the COM port.

## AetherSDR Connects But Values Look Wrong

Mock values are intentionally simple. Real KPA500/KAT500 parsing remains unverified until hardware captures are added.


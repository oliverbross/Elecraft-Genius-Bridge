# Windows Operational Guidance

## Power Settings

- Use the Windows high performance or ultimate performance power plan where appropriate.
- Disable sleep and hibernate for the bridge host.
- Disable display sleep only if the station workflow requires local visibility.
- Keep Windows Update restart windows away from operating periods.

## USB And COM Stability

- Disable USB selective suspend:
  - Control Panel -> Power Options -> Change plan settings -> Advanced power settings -> USB settings.
- Prefer direct motherboard USB ports over hubs for KPA500/KAT500 serial adapters.
- If a hub is required, use a powered hub.
- Avoid changing USB ports after Windows has assigned stable COM numbers.
- In Device Manager, check USB serial adapter power-management tabs and disable "Allow the computer to turn off this device to save power" when present.
- Record known-good mappings:
  - KPA500: `COM21` at `38400`
  - KAT500: `COM8` at `38400`

## Firewall

- For macOS AetherSDR LAN testing, allow inbound TCP only on configured PGXL/TGXL ports from the LAN profile.
- Do not create public profile rules for WAN exposure.
- Keep metrics bound to `127.0.0.1`.

## Watchdog Restart

Until a real Windows service is implemented, use Task Scheduler for unattended read-only soak tests:

- Trigger: At startup or on logon.
- Action: run `scripts\windows\run-hardware-readonly.ps1`.
- Settings: restart on failure after a delay.
- Stop task if it runs longer than the planned validation window unless intentionally soaking.

## Soak-Test Practice

Run the bridge in read-only mode first:

```powershell
cargo run -p egb -- soak-test --config config.hardware-readonly.yaml --duration-hours 4
```

Save:

- console output
- `logs/serial` transcripts
- `/status` snapshots if metrics are enabled
- AetherSDR screenshots when connected

Do not run RF-risk control tests unattended.

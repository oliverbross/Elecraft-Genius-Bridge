# Run On Windows

Create a config:

```powershell
Copy-Item config.example.yaml config.yaml
notepad config.yaml
```

Run config validation:

```powershell
.\target\release\egb.exe check-config --config .\config.yaml
```

Run the bridge:

```powershell
.\target\release\egb.exe run --config .\config.yaml
```

For first tests, leave both devices in mock mode. When real hardware is connected, set `mock: false` for the relevant device and configure its COM port.

## Helper Scripts

From the repository root:

```powershell
scripts\windows\check.ps1
scripts\windows\run-mock.ps1
scripts\windows\run-hardware-readonly.ps1
scripts\windows\run-hardware-control-local.ps1
```

Use `run-hardware-readonly.ps1` before any local control profile. It uses `config.hardware-readonly.yaml`, where both real devices have `dry_run: true`.

## Soak Test

For long-duration read-only validation:

```powershell
cargo run -p egb -- soak-test --config config.hardware-readonly.yaml --duration-hours 4
```

The command starts the normal bridge runtime and prints one health summary per minute. Keep `dry_run: true` until read-only soak results are stable.

## Scheduled Task Placeholder

A production Windows service installer is a later phase. For early station testing, use Task Scheduler:

- Trigger: at startup or at logon.
- Action: start `egb.exe`.
- Arguments: `run --config C:\path\to\config.yaml`.
- Start in: directory containing the config and logs.
- Enable restart-on-failure in Task Scheduler for read-only validation runs.

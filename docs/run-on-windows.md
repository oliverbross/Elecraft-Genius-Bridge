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

## Scheduled Task Placeholder

A production Windows service installer is a later phase. For early station testing, use Task Scheduler:

- Trigger: at startup or at logon.
- Action: start `egb.exe`.
- Arguments: `run --config C:\path\to\config.yaml`.
- Start in: directory containing the config and logs.


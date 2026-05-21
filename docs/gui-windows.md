# Windows GUI

`egb-gui.exe` is a Windows-first desktop companion for Elecraft Genius Bridge. It uses Rust `egui`/`eframe`, not Electron.

The normal GUI now uses a simple operational flow with three pages:

- Dashboard
- Setup
- Support

Advanced protocol labs, raw profiles, and detailed diagnostics are hidden until **Advanced Diagnostics** is enabled in the sidebar.

## Build

From PowerShell:

```powershell
.\scripts\windows\build-gui.ps1
```

Outputs:

```text
target-msvc\release\egb.exe
target-msvc\release\egb-gui.exe
```

The GUI expects `egb.exe` beside `egb-gui.exe`. The build script builds both binaries into the same release directory and also prepares:

```text
dist\ElecraftGeniusBridge-Windows\
```

## Run

```powershell
.\target-msvc\release\egb-gui.exe
```

Default config loaded by the GUI:

```text
config.flex-injection-readonly.yaml
```

## Normal Workflow

1. Open `egb-gui.exe`.
2. Use **Setup** to select the KPA500 COM port, KAT500 COM port, Flex radio IP, and this PC's LAN IP.
3. Choose one operating level:
   - **Safe mode**: monitor only.
   - **Operational mode**: KAT500 Tune and KPA500 Standby can be enabled.
   - **RF-risk mode**: KPA500 Operate can be enabled only with explicit confirmation.
4. Click **Save Setup**.
5. Go to **Dashboard** and click **Start Operational Bridge**.
6. Watch the top banner and readiness cards.
7. Use **Support** -> **Create Support Bundle** when you need to send evidence for analysis.

## What The GUI Does

- Edits and validates the bridge YAML config.
- Starts, stops, and restarts `egb.exe run --config <file>`.
- Starts `egb.exe evidence-test --duration-minutes 10` for SmartSDR stability captures.
- Polls `GET /status` every 500-1000 ms.
- Shows live KPA500, KAT500, Flex injection, PGXL/TGXL client, runtime, reconnect, stale-state, and poll-latency state.
- Lists serial ports.
- Runs read-only KPA500/KAT500 tests through the existing CLI.
- Runs a KPA500 `^RVM;` serial probe for busy-port/timeout diagnosis.
- Captures the last 500 GUI/bridge log lines.
- Exports ZIP diagnostics bundles under `diagnostics\`.
- Shows the latest evidence bundle path, current run directory, captured warning/error count, and SmartSDR tuner flap count.
- Shows command visibility for Flex API, PGXL direct, and TGXL direct control paths.
- Provides command simulator buttons for Tune, Standby, Operate, and Flex amplifier set. These validate EGB mapping and safety policy without depending on AetherSDR button enablement.
- Stores GUI settings in `egb-gui-settings.yaml`.

## Safety Defaults

- `allow_rf_risk` remains disabled by default.
- The GUI refuses to save or start with `kpa500.allow_rf_risk: true` until the warning acknowledgement checkbox is selected.
- KPA500 standby is available when safe controls are enabled.
- KPA500 operate requires RF-risk controls plus a per-click confirmation path and uses `test-kpa-operate`, which immediately rolls back to standby.
- KAT500 tune, bypass, and antenna controls remain disabled until control mappings are explicitly validated.
- Clear-fault remains disabled by default.

## KPA500 Troubleshooting

Enable **Advanced Diagnostics**, then use the KPA500 probe or serial diagnostics.

Expected working response:

```text
^RVM01.54;
```

If the probe times out or the COM port fails to open:

- Close Elecraft KPA500 Remote.
- Stop EGB if already running.
- Close terminal serial probes or serial monitors.
- Confirm the COM number in Windows Device Manager.
- Retry the probe before enabling any control path.

## Limitations

- The GUI is not yet an installer or Windows service manager.
- It polls the local `/status` endpoint; metrics must be enabled for live dashboard data.
- Live radio-side Flex meter value streaming is still unverified.
- Native system tray integration is not wired in this phase. Service-like settings and autostart scripts are present; see `docs/tray-autostart.md`.

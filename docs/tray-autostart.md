# Tray And Autostart

Phase 21 adds service-like GUI settings and Windows login autostart scripts. It does not yet implement a native system tray icon/menu because that should be done with a dedicated tray event-loop integration and tested on the target Windows desktop.

## Autostart

Install current-user GUI autostart:

```powershell
.\scripts\windows\install-gui-autostart.ps1
```

Remove it:

```powershell
.\scripts\windows\remove-gui-autostart.ps1
```

The scripts write/remove:

```text
HKCU\Software\Microsoft\Windows\CurrentVersion\Run\ElecraftGeniusBridgeGui
```

## GUI Settings

The GUI persists:

- Start bridge when GUI launches
- Start minimized to tray
- Close to tray
- Redact diagnostics export

These settings live in:

```text
egb-gui-settings.yaml
```

Native tray menu items planned for a later pass:

- Show
- Start Bridge
- Stop Bridge
- Restart Bridge
- Open Logs
- Export Diagnostics
- Exit

## Future Windows Service Mode

The bridge daemon should eventually run as a proper Windows service with recovery policy. The GUI should become a controller/status client instead of the process owner.

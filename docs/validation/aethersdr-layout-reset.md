# AetherSDR Applet Layout Reset

Do not delete settings automatically. Use this only as a manual diagnostic step.

## Settings Location

From AetherSDR `AppSettings`:

```text
macOS/Linux: ~/.config/AetherSDR/AetherSDR.settings
Windows: %LOCALAPPDATA%\AetherSDR\AetherSDR.settings
```

On macOS, open Terminal and back up first:

```bash
cp ~/.config/AetherSDR/AetherSDR.settings ~/.config/AetherSDR/AetherSDR.settings.phase7-backup
```

## Applet-Related Keys

Look for these XML elements:

```text
Applet_TUN
Applet_AMP
AppletOrder
AppletPanelVisible
AppletPanelFloating
AppletPanelDockedLeft
FloatingApplet_TUN_IsFloating
FloatingApplet_AMP_IsFloating
AppletPanelFloatGeometry
```

## Safe Reset Procedure

1. Quit AetherSDR.
2. Back up `AetherSDR.settings`.
3. Edit only applet-related keys.
4. Prefer setting:

```text
AppletPanelVisible=True
AppletPanelFloating=False
Applet_TUN=True
Applet_AMP=True
```

5. Remove `FloatingApplet_TUN_IsFloating`, `FloatingApplet_AMP_IsFloating`, and stale float geometry only if the applet panel appears off-screen.
6. Start AetherSDR.
7. Reconnect TGXL direct IP and observe whether TUN appears.

This reset cannot create PGXL amplifier presence by itself. It only rules out stale applet layout state.

# AetherSDR Applet Layout Reset

Use this when direct PGXL/TGXL sockets connect and poll but the AetherSDR applet tray may be stale, collapsed, or persisted in a hidden/floating layout.

This does not create device presence. It only removes layout preferences so AetherSDR can show present applets normally.

## Source-Confirmed Settings Paths

The inspected AetherSDR source stores the main settings file at:

```text
~/.config/AetherSDR/AetherSDR.settings
```

Older macOS builds may have migrated from:

```text
~/Library/Preferences/AetherSDR/AetherSDR/AetherSDR.settings
```

The in-app Support reset also removes:

```text
~/Library/Preferences/com.aethersdr.AetherSDR.plist
```

## Safe Layout-Only Reset

Quit AetherSDR first.

Dry-run:

```sh
scripts/aethersdr/reset-applet-layout.sh
```

Apply:

```sh
scripts/aethersdr/reset-applet-layout.sh --apply
```

The script backs up settings files before editing and removes only applet/layout keys:

```text
Applet_TUN
Applet_AMP
AppletOrder
AppletPanelVisible
AppletPanelFloating
AppletPanelFloatGeometry
FloatingApplet_TUN_IsFloating
FloatingApplet_AMP_IsFloating
FloatingApplet_TUN_Geometry
FloatingApplet_AMP_Geometry
```

## Full AetherSDR Reset

AetherSDR also has an in-app Support reset that removes all app-specific settings and quits the app. Use that only if the layout-only reset does not resolve a confirmed-present applet.

The full reset does not change settings stored on the radio, but it may remove AetherSDR UI preferences beyond applet layout.

## Expected Result

After reset:

- `TUN` should appear if the installed AetherSDR binary supports direct TGXL presence and the TGXL direct socket is connected.
- `AMP` still will not appear from direct PGXL TCP alone in the inspected source. It requires radio-side amplifier presence or an AetherSDR patch/proxy path.

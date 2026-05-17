# PGXL Applet Non-Proxy Options

Status: source-inspected, not radio-configuration validated.

## Current Source Finding

AetherSDR AMP applet visibility is driven by `RadioModel::amplifierChanged(true)`. That signal is emitted only when AetherSDR receives a Flex radio API status object:

```text
amplifier <handle> model=<non-empty, not TunerGeniusXL> ...
```

The direct PGXL socket can connect and feed telemetry, but it does not set `m_hasAmplifier` and does not call `AppletPanel::setAmpVisible(true)`.

## Option 1: Flex Radio Native PGXL Configuration

May work if the Flex radio can be configured to know about a PGXL and emit normal `amplifier` status records with an `ip` field.

Evidence needed:

- AetherSDR transcript from the Flex API after `sub amplifier all`.
- Radio UI/SmartSDR setting that configures PGXL IP/model.
- Status line similar to:

```text
S...|amplifier <handle> model=PowerGeniusXL ip=<egb-ip> state=STANDBY ...
```

This is the preferred non-proxy path if available because it works through normal AetherSDR logic and should be SmartLink-friendly.

## Option 2: AetherSDR Setting To Show AMP Manually

No source evidence found for a manual "always show AMP applet" setting. `Applet_AMP` controls whether a present AMP applet is open/checked, but `applyConditionalPresence` still hides the button when amplifier presence is false.

Conclusion: unlikely to solve PGXL visibility alone.

## Option 3: Applet Layout Reset

May solve stale UI state, but not missing PGXL presence. It is still worth testing because TUN should be direct-presence capable in the inspected source.

Relevant settings keys:

```text
Applet_AMP
Applet_TUN
AppletOrder
AppletPanelVisible
AppletPanelFloating
AppletPanelDockedLeft
FloatingApplet_AMP_IsFloating
FloatingApplet_TUN_IsFloating
```

Settings file path from `AppSettings`:

```text
macOS/Linux: ~/.config/AetherSDR/AetherSDR.settings
Windows: %LOCALAPPDATA%\AetherSDR\AetherSDR.settings
```

Do not delete this file blindly. Back it up first, then either use the app's reset/reorder controls or edit/remove only the applet-related keys while AetherSDR is closed.

## Option 4: Fake Amplifier Entry Via Radio Config

Unknown. If Flex firmware allows defining a PGXL by IP without verifying real PGXL hardware, point it at the EGB Windows LAN IP and then check whether AetherSDR receives `amplifier` status.

This needs real Flex radio/SmartSDR validation.

## Option 5: SmartSDR/AetherSDR Accessory Profile Files

No source evidence found that AetherSDR reads an accessory profile file capable of creating a PGXL amplifier model entry. AetherSDR appears to rely on live radio status plus its own app settings.

## Decision Point

Try in this order:

1. Reset applet layout state safely.
2. Confirm whether native Flex/SmartSDR config can advertise a PGXL IP.
3. If native radio config works, keep EGB direct-only.
4. If native radio config cannot create `amplifier` status and AMP UI is required without modifying AetherSDR, design optional Flex API proxy/injection.

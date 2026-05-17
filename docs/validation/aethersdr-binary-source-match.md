# AetherSDR Binary / Source Match Checklist

Use this when AetherSDR connects to EGB but the expected applet button does not appear.

## Identify The Running Binary

Capture:

- AetherSDR About dialog version, build number, and commit if shown.
- macOS version.
- Whether AetherSDR was installed from a release build, package, or locally built source.
- Screenshot of the About dialog.
- Screenshot of Radio Setup -> Peripherals showing PGXL/TGXL connected.

## Compare Against Inspected Source

EGB has inspected local AetherSDR source at:

```text
research/AetherSDR
```

The inspected commit is:

```text
6d17b3bbda96b836762e7d40758a1fc3e14725f9
```

From this repo, run:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\inspect-aethersdr-source.ps1 -Path research\AetherSDR
```

For TGXL direct applet visibility, the critical source evidence is:

```text
TunerModel::m_directPresence
TunerModel::isPresent() returns !m_handle.isEmpty() || m_directPresence
TunerModel::setDirectConnection(TgxlConnection*)
MainWindow wires TunerModel::presenceChanged to AppletPanel::setTunerVisible
```

If the running binary was built from a source revision that does not contain these, direct TGXL can connect and poll while the TUN applet stays hidden.

## What To Capture

- PGXL protocol transcript from `logs/protocol`.
- TGXL protocol transcript from `logs/protocol`.
- AetherSDR log if available.
- Screenshot of applet tray before and after TGXL direct connection.
- Screenshot showing whether the right-side applet panel itself is visible.
- Screenshot of Peripherals tab connected state.
- Any applet layout or reset action attempted.

## Likely Reasons TUN Remains Hidden

- The macOS binary predates the `m_directPresence` fallback.
- The tested binary differs from `research/AetherSDR`.
- The TGXL connection is established in the Peripherals dialog but not through `MainWindow::m_tgxlConn`.
- The applet panel is hidden, floating off-screen, or layout state suppresses the button.
- The `Applet_TUN` setting is false or stale.
- The bridge was connected before the main applet wiring was initialized and the binary has no reconnect/presence refresh path.

## Decision

If the binary includes `m_directPresence`, direct TGXL should be able to show TUN. Test layout reset before considering any proxy work.

If the binary does not include `m_directPresence`, either update AetherSDR or use a future Flex API presence path.

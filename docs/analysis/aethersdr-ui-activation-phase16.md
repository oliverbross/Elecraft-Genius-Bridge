# AetherSDR UI Activation Phase 16

Status: source-inspected against local `research/AetherSDR` checkout at commit `6d17b3bbda96b836762e7d40758a1fc3e14725f9`.

## Confirmed Gating Code

`src/gui/AppletPanel.cpp` creates `TUN` and `AMP` applets at startup, but both tray buttons are hidden by default:

```text
makeEntry("TUN", "Tuner", ..., defaultOn=false); m_tuneBtn->hide();
makeEntry("AMP", "Amplifier", ..., defaultOn=false); m_ampBtn->hide();
```

The only direct visibility controls are:

```text
AppletPanel::setTunerVisible(bool visible)
AppletPanel::setAmpVisible(bool visible)
```

Both call `applyConditionalPresence`. When `visible=false`, AetherSDR hides the tray button and unchecks it. When `visible=true`, it shows the tray button and restores the saved applet preference from `Applet_TUN` or `Applet_AMP`.

Consequence: applet layout settings can keep a present applet closed, but they cannot force a missing applet to appear. Presence must be true first.

## TUN / TGXL Activation

`src/gui/MainWindow.cpp` wires:

```text
TunerModel::presenceChanged -> AppletPanel::setTunerVisible
TunerModel::presenceChanged -> TGXL indicator visible
TunerModel::setDirectConnection(&m_tgxlConn)
```

`src/models/TunerModel.h` implements:

```text
isPresent() = !m_handle.isEmpty() || m_directPresence
```

`src/models/TunerModel.cpp` sets `m_directPresence=true` when `TgxlConnection::connected` fires and emits `presenceChanged(true)` if no radio-side tuner handle already existed.

The inspected source therefore supports TUN applet visibility from direct TGXL TCP alone. This support is explicitly called out in generated release notes as:

```text
0.9.5 / 2026-05-02: "TGXL detected via direct TCP only (#2250, chrisb1964)"
```

If Oliver's installed macOS binary connects to TGXL direct TCP but still does not show `TUN`, the highest-probability causes are:

- installed binary predates the 0.9.5 direct-presence fix,
- installed binary differs from inspected source,
- direct connection is made through the setup dialog but not through `MainWindow::m_tgxlConn`,
- applet panel/layout state is hiding a present applet after presence is established,
- or TGXL disconnects before the presence signal persists.

## AMP / PGXL Activation

`src/gui/MainWindow.cpp` wires:

```text
RadioModel::amplifierChanged -> AppletPanel::setAmpVisible
RadioModel::amplifierChanged -> PGXL indicator visible
PgxlConnection::statusUpdated -> AmpApplet telemetry only
```

`src/models/RadioModel.cpp` sets amplifier presence only from Flex API radio status:

```text
S<radio>|amplifier <handle> model=<non-empty non-TunerGeniusXL> ...
```

For PGXL-like behavior the intended shape is:

```text
S<radio>|amplifier <handle> model=PowerGeniusXL ip=<egb-ip> state=STANDBY ...
```

Direct PGXL TCP connection does not emit `amplifierChanged(true)`. It can feed telemetry after the applet exists, but it cannot create or show the AMP applet in the inspected source.

Operate/standby also depends on a radio-side amplifier handle:

```text
amplifier set <handle> operate=<0|1>
```

The inspected AetherSDR source does not expose a direct PGXL-only operate path for the AMP applet.

## Manual Show Settings

AetherSDR has a `View -> Reset Applet Order` action and persisted applet state keys, but no source-visible manual override that can make `AMP` or `TUN` appear without presence.

Useful settings keys:

```text
Applet_TUN
Applet_AMP
AppletOrder
AppletPanelVisible
AppletPanelFloating
AppletPanelFloatGeometry
FloatingApplet_TUN_IsFloating
FloatingApplet_AMP_IsFloating
```

Setting `Applet_AMP=True` or `Applet_TUN=True` only controls whether the applet opens after presence has already made the tray button visible.

## Binary/Source Match Check

On the macOS test machine:

1. Open AetherSDR and record the visible version from About or title bar.
2. If it is older than `0.9.5`, assume it does not contain the direct TGXL presence fix.
3. If possible, check the app binary strings:

```sh
strings /Applications/AetherSDR.app/Contents/MacOS/AetherSDR | grep -E "TGXL detected via direct TCP|direct TGXL connection established|m_directPresence|0\\.9\\.5"
```

4. Check logs for:

```text
TunerModel: direct TGXL connection established
TunerModel: direct TGXL connection lost
```

If the direct connection is visible in EGB transcripts but no AetherSDR log contains the direct TGXL model message, the installed binary likely lacks the direct-presence path or the setup path is not connected to `MainWindow::m_tgxlConn`.

## Recommendation

For immediate next validation:

1. Verify the installed macOS AetherSDR binary is `0.9.5` or newer, or otherwise contains the #2250 direct TGXL presence fix.
2. Run the layout reset helper in dry-run mode, then apply it only after quitting AetherSDR.
3. Retest TGXL direct TCP with `config.aethersdr-compat-readonly.yaml`.

For PGXL applet visibility:

- direct PGXL sockets alone are not enough in the inspected source;
- if Flex radio configuration can make the radio emit a real `amplifier ... model=PowerGeniusXL` record, prefer that;
- if stock AetherSDR compatibility is required and the radio cannot advertise the amp, implement the minimal Flex API amplifier-status injection path;
- if maintaining a custom AetherSDR build is acceptable, a small AetherSDR patch adding PGXL direct presence is lower operational risk than proxying the radio API.

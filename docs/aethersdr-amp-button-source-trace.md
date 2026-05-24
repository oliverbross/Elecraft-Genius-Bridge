# AetherSDR AMP Button Source Trace

Reviewed source tree: `.tmp/AetherSDR`.

## Visible Button

File: `.tmp/AetherSDR/src/gui/AmpApplet.cpp`

- The Amp applet creates `m_operateBtn`.
- The button is hidden by default.
- `AmpApplet::setState()` shows the button whenever PGXL/Flex telemetry provides
  a `state` value.
- The label is state-derived:
  - `IDLE`, `OPERATE`, or `TRANSMIT*` -> green `OPERATE`.
  - other states, including `STANDBY` -> `STANDBY`.

The visible button is command-capable in this source tree. It is not merely a
label.

## Click Signal

File: `.tmp/AetherSDR/src/gui/AmpApplet.cpp`

Clicking `m_operateBtn` runs:

```cpp
bool isOp = (m_operateBtn->text() == "OPERATE");
emit operateToggled(!isOp);
```

Meaning:

- If displayed `OPERATE`, click emits `operateToggled(false)`.
- If displayed `STANDBY`, click emits `operateToggled(true)`.

## Command Emission

File: `.tmp/AetherSDR/src/gui/MainWindow.cpp`

The signal is handled as:

```cpp
if (!m_radioModel.ampHandle().isEmpty())
    m_radioModel.sendCommand(
        QString("amplifier set %1 operate=%2")
            .arg(m_radioModel.ampHandle())
            .arg(on ? 1 : 0));
```

So the actual command path is Flex API:

`amplifier set <handle> operate=<0|1>`

The inspected AetherSDR PGXL direct TCP class does not send direct PGXL
standby/operate commands. It sends `info` and repeated `status`.

## Required Conditions

From source:

- The Amp applet must receive a `state` field so the button is visible.
- `RadioModel::ampHandle()` must be non-empty at click time.
- `ampHandle` comes from Flex `amplifier <handle> ... model=PowerGeniusXL ...`
  status.
- PGXL direct TCP connection is not the command gate for the button, although it
  feeds telemetry.

Not required by the inspected click path:

- Non-zero meter values.
- Interlock `tx_allowed`.
- Direct PGXL command capability fields.
- PGXL direct control commands.

## EGB Interpretation

If EGB evidence shows:

- `flex_injection.amplifier_handle` is non-empty,
- PGXL direct status is stable,
- `flex-control-commands.log` is empty,
- `pgxl-control-commands.log` is empty,

then the installed AetherSDR build did not emit the command for that user action.
The most likely explanations are:

- the clicked UI element was a peripheral/status element rather than the Amp
  applet button;
- the installed binary differs from the inspected source;
- the runtime radio model did not have `m_ampHandle` at the exact click moment.

EGB cannot execute KPA500 standby/operate without receiving either the Flex
`amplifier set` command or a direct PGXL control command.

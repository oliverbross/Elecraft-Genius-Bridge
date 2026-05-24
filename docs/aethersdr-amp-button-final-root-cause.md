# AetherSDR AMP Button Final Root Cause

Phase 67 confirms the EGB side of the AMP control path is ready, but the
installed AetherSDR session still does not emit a control command.

## EGB Evidence

Observed in live evidence:

- PGXL direct TCP connects immediately.
- PGXL direct receives `info` and repeated `status`.
- `pgxl-control-commands.log` is empty.
- `flex-control-commands.log` is empty.
- `simulate-pgxl-control standby` maps to KPA500 `^OS0;` correctly when a
  command exists.
- Flex amplifier handle is present and stable in EGB status during healthy runs.

## AetherSDR Source Path

The inspected source shows the Amp applet button is command-capable:

1. `AmpApplet::setState()` shows `m_operateBtn`.
2. Clicking the button emits `operateToggled(bool)`.
3. `MainWindow` handles the signal and sends:

`amplifier set <handle> operate=<0|1>`

4. The command is sent only when `RadioModel::ampHandle()` is non-empty.

The inspected direct PGXL TCP class sends `info` and `status`; it does not send
operate/standby control over direct PGXL TCP.

## Conclusion

If `flex-control-commands.log` remains empty while the EGB status shows a stable
Flex amplifier handle, the installed AetherSDR build/user action did not reach
the command-emitting source path. EGB cannot switch KPA500 standby/operate
without receiving either:

- Flex API `amplifier set <handle> operate=<0|1>`, or
- a direct PGXL control command such as `standby` / `operate`.

## Recommended AetherSDR Diagnostic Patch

Patch AetherSDR locally to log:

- `AmpApplet::operateToggled(bool)` emission.
- `RadioModel::ampHandle()` at click time.
- `RadioModel::ampIp()` at click time.
- whether `MainWindow` sends `amplifier set`.
- whether any UI state suppresses the click.

Minimal target files:

- `src/gui/AmpApplet.cpp`
- `src/gui/MainWindow.cpp`
- `src/models/RadioModel.cpp`

This patch is the next step if the visible AMP applet button still appears
unresponsive after EGB reports a stable amplifier handle.

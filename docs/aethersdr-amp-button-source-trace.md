# AetherSDR AMP Button Source Trace

Phase 74 instrumentation target: prove whether the AetherSDR AMP UI emits a Flex amplifier control command.

## Source Path

- `.tmp/AetherSDR/src/gui/AmpApplet.cpp`
  - The AMP applet creates `m_operateBtn`.
  - A click computes the next requested state and emits `operateToggled(bool)`.
- `.tmp/AetherSDR/src/gui/MainWindow.cpp`
  - `MainWindow` connects `AmpApplet::operateToggled` to a lambda.
  - The lambda sends `amplifier set <handle> operate=<0|1>` only when `RadioModel::ampHandle()` is non-empty.
  - The bottom `m_pgxlIndicator` click calls `RadioModel::setAmpOperate(!ampOperate())`.
- `.tmp/AetherSDR/src/models/RadioModel.cpp`
  - `RadioModel::setAmpOperate(bool)` sends the same `amplifier set <handle> operate=<0|1>` command only when `m_ampHandle` is non-empty.
  - Amplifier status parsing sets `m_ampHandle`, `m_ampIp`, `m_hasAmplifier`, and `m_ampOperate`.

## Diagnostic Log

Temporary instrumentation writes to:

```text
aethersdr-amp-diagnostic.log
```

The log records:

- `AmpApplet button_clicked`: the visible button was clicked and `operateToggled` was emitted.
- `MainWindow operateToggled received`: the signal reached the command handler.
- `MainWindow operateToggled skipped reason=empty_ampHandle`: no command was sent because AetherSDR had no amplifier handle.
- `MainWindow operateToggled sending_flex_command=...`: AetherSDR emitted the Flex command.
- `RadioModel power_amplifier_detected`: the radio-side amplifier parser observed a PGXL amplifier and stored handle/IP.
- `RadioModel amplifier_status`: current radio-side amplifier state and handle.
- `RadioModel setAmpOperate ...`: bottom PGXL indicator control path.

## Current Interpretation

If `flex-control-commands.log` and `pgxl-control-commands.log` stay empty in EGB while `aethersdr-amp-diagnostic.log` shows `sending_flex_command=...`, the command left AetherSDR and EGB/Flex capture must be rechecked.

If the diagnostic log shows the click but `skipped reason=empty_ampHandle`, the AetherSDR UI is command-capable but not eligible because it has no radio-side amplifier handle.

If the diagnostic log shows no `button_clicked`, the user action did not hit the AMP applet button path; it was likely a status/display element or a disabled UI path.

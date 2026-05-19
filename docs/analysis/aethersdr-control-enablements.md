# AetherSDR Control Enablement Notes

Source inspected: `research/AetherSDR` commit `6d17b3bbda96b836762e7d40758a1fc3e14725f9`.

## TUN / TGXL Controls

`src/gui/TunerApplet.cpp` creates the TUN applet Tune button and wires clicks directly to `TunerModel::autoTune()`. The button is not explicitly disabled by the applet code.

`src/models/TunerModel.cpp` chooses the direct TGXL TCP path first:

- if the direct port 9010 connection is connected, `autoTune()` sends direct command `autotune`
- otherwise, if a radio-side tuner handle exists, it sends Flex command `tgxl autotune handle=<handle>`

Antenna buttons are visible only when both conditions are true:

- direct TGXL connection is connected
- `one_by_three=1` has been observed

Manual relay buttons use direct TGXL commands:

- `tune relay=0 move=<+1|-1>`
- `tune relay=1 move=<+1|-1>`
- `tune relay=2 move=<+1|-1>`

The TX applet ATU button is separate from the TUN applet. In `src/gui/TxApplet.cpp`, the internal radio ATU and MEM controls are disabled when:

```text
tuner->isPresent() && tuner->isOperate() && !tuner->isBypass()
```

This means `operate=1 bypass=0` intentionally disables the radio ATU button because AetherSDR considers the external TGXL active. It does not disable the TUN applet Tune button.

## AMP / PGXL Controls

`src/gui/AmpApplet.cpp` shows the operate button when amplifier telemetry updates call `AmpApplet::setState()`. The button text/style is based on PGXL state:

- operating: `IDLE`, `OPERATE`, or any `TRANSMIT*`
- standby/non-operating: other states such as `STANDBY`

The AMP button does not send PGXL direct TCP commands. `src/gui/MainWindow.cpp` and `src/models/RadioModel.cpp` send the Flex radio command:

```text
amplifier set <handle> operate=<0|1>
```

Therefore EGB must observe the resulting Flex amplifier status update and map `operate=0` to KPA500 `^OS0;`, and `operate=1` to KPA500 `^OS1;` only when RF-risk controls are explicitly enabled.

## Phase 27 Corrections

- Flex amplifier reannounce now uses live KPA500 state instead of hard-coded `state=STANDBY`.
- Flex amplifier telemetry `state=...` is no longer treated as a control request. Only explicit `operate=<0|1>` is mapped to desired KPA500 control.
- `/status.controls` exposes the last observed PGXL/TGXL/Flex control command and safety decision.
- `egb control-lab` captures a button-press window and writes `controls-analysis.md`, `control-events.jsonl`, and per-protocol control logs.

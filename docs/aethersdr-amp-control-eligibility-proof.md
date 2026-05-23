# AetherSDR AMP Control Eligibility Proof

Phase 61 source review shows the AMP Standby/Operate path is not a PGXL direct TCP command path in the inspected AetherSDR source.

## Source Findings

- `research/AetherSDR/src/gui/AmpApplet.cpp` emits `operateToggled(...)` when the AMP applet button is clicked.
- `research/AetherSDR/src/gui/MainWindow.cpp` handles that signal by sending a Flex radio command only when `m_radioModel.ampHandle()` is non-empty:

```text
amplifier set <amp-handle> operate=<0|1>
```

- `research/AetherSDR/src/models/RadioModel.cpp` populates the AMP handle from radio-side `amplifier <handle> ... model=PowerGeniusXL ...` status lines.
- `research/AetherSDR/src/core/PgxlConnection.cpp` uses the direct PGXL TCP socket for `info` and repeated `status` polling. No operate/standby direct PGXL command path is present in the inspected source.

## Practical Conclusion

If EGB sees no line in `flex-control-commands.log`, no line in `pgxl-control-commands.log`, and `/status.controls.last_flex_amp_set_command` remains `null`, then AetherSDR did not emit a control command. EGB cannot send KPA500 `^OS0;` or `^OS1;` without that client command.

The most likely gates are therefore inside AetherSDR before the packet is sent:

- the installed binary has no valid radio-side amplifier handle at click time,
- the button click is not reaching `AmpApplet::operateToggled`,
- the applet is visible in a display-only state for this connection mode,
- or the installed macOS/Windows binary differs from the inspected source.

## PGXL Status Experiments

The `pgxl.status_profile` variants intentionally change only direct PGXL `info`/`status` fields. They do not change Flex amplifier create fields, KPA500 polling, or safety gates.

- `status_current`: current proven status body.
- `status_control_fields`: appends `operate_capable=1 standby_capable=1`.
- `status_operate_capable`: backward-compatible alias of the same capability flags.
- `status_rich_metered`: appends capability flags and duplicate meter-style fields.
- `status_realistic_operate`: forces direct PGXL status state to `OPERATE` and appends rich fields for an eligibility test only.
- `status_realistic_standby`: forces direct PGXL status state to `STANDBY` and appends rich fields for an eligibility test only.
- `status_real_pgxl_like`: appends model, serial, firmware, and capability flags.

A profile only proves useful if the evidence bundle shows a real command arrival in `flex-control-commands.log`, `pgxl-control-commands.log`, or `control-events.jsonl`.

## Local Mapping Simulator

Use this to prove the EGB side would map the command correctly if AetherSDR sent it:

```powershell
.\target\release\egb.exe simulate-pgxl-control --config .\config.aethersdr-last-known-good-real-controls.yaml --command standby
.\target\release\egb.exe simulate-pgxl-control --config .\config.aethersdr-last-known-good-real-controls.yaml --command operate
```

`standby` should map to KPA500 `^OS0;` when safe controls are enabled. `operate` must remain blocked unless RF-risk mode is explicitly enabled.

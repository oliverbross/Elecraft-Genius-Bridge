# AetherSDR AMP Button Gating

Latest evidence shows AetherSDR opens PGXL direct TCP and polls status, but AMP Standby/Operate button presses may not emit a Flex `amplifier set ... operate=<0|1>` command or PGXL direct control command.

Likely gating inputs to validate:

- PGXL direct status `state`.
- Flex amplifier status `state`.
- Interlock `tx_allowed`.
- Presence of AMP meter handles and whether SmartSDR/AetherSDR receives live meter values.
- Whether the applet treats `STANDBY` as display-only until an operate-capable radio-side state exists.

Current EGB diagnostics:

- `control-events.jsonl`
- `flex-control-commands.log`
- `pgxl-control-commands.log`
- `/status.controls.last_flex_amp_set_command`
- `/status.controls.last_pgxl_control_command`

If no command appears in these files after a button press, the button is gated client-side. The next live test should compare normal interlock mode vs `flex_injection.disable_amp_interlock=true` to determine whether the Flex interlock is suppressing the control path.

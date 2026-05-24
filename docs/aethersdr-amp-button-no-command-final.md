# AetherSDR AMP Button No-Command Final

Phase 63 evidence confirms the AMP Standby/Operate problem is command emission, not EGB command execution.

Observed:

- `simulate-pgxl-control --command standby` maps to KPA500 `^OS0;`.
- Effective policy allows standby when the real-controls profile enables KPA standby.
- During live AetherSDR button tests:
  - `pgxl-control-commands.log` remains empty.
  - `flex-control-commands.log` remains empty.
  - `control_requested_count=0`.
  - `aethersdr_button_command_seen=false`.

Therefore AetherSDR is not sending a command that EGB can execute.

## Source Finding

The inspected AetherSDR source routes AMP controls through the Flex radio API.
`AmpApplet::setState()` shows the button when direct or fallback amplifier
telemetry includes a `state` field. Clicking the button emits `operateToggled`.
`MainWindow` handles that signal by sending:

```text
amplifier set <handle> operate=<0|1>
```

That command is sent only if the runtime radio model has a non-empty amplifier
handle. The direct PGXL TCP path is used for `info` and `status` telemetry; the
inspected source does not send operate/standby over direct PGXL TCP.

## Likely Explanations

- The installed AetherSDR binary treats the visible AMP button as state display in this path.
- The command is available only through another UI/control mode.
- AetherSDR runtime has no valid `m_ampHandle` at the exact click time, or the
  click is occurring on a peripheral connection UI element rather than the Amp
  applet button.
- AetherSDR internally gates the button before emitting `operateToggled`.

EGB cannot execute KPA500 `^OS0;` or `^OS1;` when no PGXL or Flex control command arrives. The next useful evidence is AetherSDR-side logging around `AmpApplet::operateToggled`, `RadioModel::ampHandle()`, and the `amplifier set` send path.

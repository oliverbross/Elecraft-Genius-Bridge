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

The inspected AetherSDR source routes AMP controls through the Flex radio API:

```text
amplifier set <handle> operate=<0|1>
```

That command is sent only if the runtime radio model has a non-empty amplifier handle. The direct PGXL TCP path is used for `info` and `status` telemetry, not for operate/standby control in the inspected source.

## Likely Explanations

- The installed AetherSDR binary treats the visible AMP button as state display in this path.
- The command is available only through another UI/control mode.
- AetherSDR runtime has no valid `m_ampHandle` at the exact click time.
- AetherSDR internally gates the button before emitting `operateToggled`.

EGB cannot execute KPA500 `^OS0;` or `^OS1;` when no PGXL or Flex control command arrives. The next useful evidence is AetherSDR-side logging around `AmpApplet::operateToggled`, `RadioModel::ampHandle()`, and the `amplifier set` send path.

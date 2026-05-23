# AetherSDR AMP Button Gating Final

Phase 62 evidence:

- `simulate-pgxl-control --command standby` maps to KPA500 `^OS0;` and is allowed when safe standby controls are enabled.
- `pgxl-control-commands.log` remains empty during AMP button tests.
- `flex-control-commands.log` remains empty during AMP button tests.
- Interlock-disabled mode did not make the AMP button emit a command.

This proves the remaining failure is not the EGB Elecraft mapping layer. AetherSDR is not sending a command.

## Inspected Source Path

There are two UI paths in the inspected AetherSDR source:

1. The AMP applet button emits `AmpApplet::operateToggled`.
2. `MainWindow.cpp` sends:

```text
amplifier set <amp-handle> operate=<0|1>
```

only if `m_radioModel.ampHandle()` is non-empty.

The status bar PGXL indicator also calls `RadioModel::setAmpOperate(!ampOperate())`, which has the same guard:

```text
if m_ampHandle is empty: return
```

The inspected `PgxlConnection` direct TCP client sends `info` and `status`; it does not send PGXL direct operate/standby commands.

## Gating Conclusion

The exact command-emission gate in inspected source is a non-empty radio-side amplifier handle. If EGB sees no Flex `amplifier set` command, then one of these is true:

- the installed AetherSDR binary does not have the same control path as the inspected source,
- the clicked UI element is not the applet/status control that emits operate toggles,
- AetherSDR's runtime radio model has lost or never populated `m_ampHandle`,
- or the UI control is display-only/disabled before the signal reaches `MainWindow`.

PGXL direct status profiles can still be tested, but source review suggests they are unlikely to create a command by themselves because the command is routed through the Flex radio amplifier handle, not the PGXL TCP socket.

## Experiment Profiles

Use only `pgxl.status_profile` for this experiment:

- `status_control_fields`
- `status_realistic_operate`
- `status_realistic_standby`
- `status_rich_metered`

Success requires hard evidence: a non-empty `flex-control-commands.log`, `pgxl-control-commands.log`, or `control-events.jsonl`.

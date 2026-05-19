# Control Lab

Use control lab when AetherSDR panes are visible but button presses appear disabled or unresponsive.

Run:

```powershell
.\target-msvc\debug\egb.exe control-lab --config .\config.flex-injection-readonly.yaml --duration-minutes 5
```

During the run, press the AetherSDR TUN and AMP controls you want to test. The evidence bundle records whether any command reached EGB:

- `controls-analysis.md`
- `control-events.jsonl`
- `tgxl-control-commands.log`
- `pgxl-control-commands.log`
- `flex-control-commands.log`
- `status-samples.jsonl`

Interpretation:

- No control events: AetherSDR did not send a command, so the issue is UI enablement or missing state fields.
- Control event with `blocked_by_dry_run`: command reached EGB and was intentionally blocked by safe dry-run configuration.
- Control event with `blocked_by_rf_risk`: operate/tune reached EGB but RF-risk controls are disabled.
- Control event with `accepted_desired_state`: command reached EGB and was mapped into the bridge desired-state layer.

Default profiles remain safe. KPA500 operate (`^OS1;`) and KAT500 tune (`T;`) remain RF-risk gated.

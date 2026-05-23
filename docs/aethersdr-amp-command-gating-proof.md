# AetherSDR AMP Command Gating Proof

Current evidence shows the AMP widget is visible and PGXL direct TCP can be stable, but button presses do not produce any command EGB can execute:

- `pgxl-control-commands.log` is empty.
- `flex-control-commands.log` is empty.
- `/status.controls.last_pgxl_control_command` is `none`.
- `/status.controls.last_flex_amp_set_command` is `none`.

That means the failure is before the Elecraft mapping layer. EGB cannot send `^OS0;` or `^OS1;` until AetherSDR emits either a PGXL direct control command or a Flex API `amplifier set ... operate=<0|1>` command.

## What To Prove

The next live runs should distinguish these cases:

- The AMP control is visually present but client-side disabled.
- AetherSDR requires additional PGXL direct status/capability fields before emitting commands.
- AetherSDR requires Flex meter values or interlock state before enabling controls.
- AetherSDR treats the visible Standby/Operate label as state-only in this pairing mode.

## PGXL Status Profile Experiments

`pgxl.status_profile` changes only direct PGXL status fields. It does not change Flex amplifier create fields, KPA500 polling, or safety gates.

- `status_current`: current proven status body.
- `status_control_fields`: appends `operate_capable=1 standby_capable=1`.
- `status_operate_capable`: appends `operate_capable=1 standby_capable=1`.
- `status_rich_metered`: appends capability flags plus duplicate meter-style fields.
- `status_realistic_operate`: forces direct PGXL status to `state=OPERATE` and appends rich fields for eligibility testing only.
- `status_realistic_standby`: forces direct PGXL status to `state=STANDBY` and appends rich fields for eligibility testing only.
- `status_real_pgxl_like`: appends model/serial/firmware and capability flags.

Run:

```powershell
.\target\release\egb.exe operational-gap-test --config .\config.aethersdr-last-known-good-real-controls.yaml --duration-minutes 5
```

Change only `pgxl.status_profile` between runs and press the AMP buttons during the test window. A successful gating breakthrough is not visual; it is hard evidence that one of these files contains a non-status control command:

- `pgxl-control-commands.log`
- `flex-control-commands.log`
- `control-events.jsonl`

If none of the status profiles produce command arrival, the remaining gating condition is outside PGXL direct status, most likely Flex-side meter/interlock/operate eligibility or AetherSDR UI logic.

See `docs/aethersdr-amp-control-eligibility-proof.md` for the source-level finding: the inspected AetherSDR source sends AMP controls via Flex `amplifier set <handle> operate=<0|1>`, not through PGXL direct TCP.

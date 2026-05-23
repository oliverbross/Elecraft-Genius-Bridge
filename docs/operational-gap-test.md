# Operational Gap Test

Use this command when AetherSDR/SmartSDR behavior needs one focused evidence bundle:

```powershell
.\target\release\egb.exe operational-gap-test --config .\config.aethersdr-last-known-good-real-controls.yaml --duration-minutes 5
```

The generated bundle includes `operational-gap-test.md` with:

- PGXL and TGXL direct session presence
- PGXL connect delay
- AMP command received yes/no
- KAT500 `F <kHz>;` requested vs confirmed kHz
- stale KAT frequency response count and retry count
- whether KAT tuning appears stuck
- KPA500 band-follow sent/skipped status
- SmartSDR PGXL meter publication status
- SmartSDR TGXL/tuner support status
- interlock mode and latest `tx_allowed`

This command does not alter safety gates. It reports whether configured controls are seen, executed, blocked, or absent.

For PGXL slow-connect timing, use the narrower strategy comparison command:

```powershell
.\target\release\egb.exe pgxl-trigger-strategy-test --config .\config.aethersdr-last-known-good-real-controls.yaml --strategy rapid_sub_only --duration-seconds 120
```

That command preserves the selected config and changes only `flex_injection.pgxl_startup_trigger_strategy` for the evidence run.

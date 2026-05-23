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

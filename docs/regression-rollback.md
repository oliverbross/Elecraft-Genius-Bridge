# Regression Rollback

Use this when AetherSDR still shows AMP/TUN panes but PGXL or TGXL direct connectivity regresses.

## Known-Good Profile

Start with:

```powershell
.\target-msvc\debug\egb.exe run --config .\config.aethersdr-known-good.yaml
```

For a timed evidence capture:

```powershell
.\target-msvc\debug\egb.exe aethersdr-smoke-test --config .\config.aethersdr-known-good.yaml --duration-minutes 5
```

The current AetherSDR-compatible operational profile uses:

- `flex_injection.amplifier_status_profile: aethersdr_operational`
- `tgxl.control_profile: control_ready`
- real Tune/Standby controls only when `operational.enable_real_controls=true`
- `allow_rf_risk: false`

Replace `server.bind_ip`, `flex_injection.radio_ip`, and `flex_injection.amplifier_ip` with the Windows LAN addresses for cross-machine tests.

## Profile Rules

- Use `aethersdr_operational` for AetherSDR PGXL direct-connect operational tests.
- Use `aethersdr_force_direct` or `old_good_pgxl` only for lab/regression comparison.
- Use `official_pgxl` or `strict_real_pgxl` only for protocol audits. They may be too conservative to trigger AetherSDR TCP 9008.
- TGXL control-button enablement requires `control_ready` or `verbose_control`; `readonly` intentionally reports a conservative direct state.

## What To Check

The evidence ZIP should include:

- `profile-summary.md`
- `last-known-good-comparison.md`
- `advertised-state-history.jsonl`
- `state-mismatch-events.jsonl`
- `pgxl-protocol.log`
- `tgxl-protocol.log`

If `state-mismatch-events.jsonl` is non-empty, fix state propagation before changing PGXL/TGXL protocol fields.

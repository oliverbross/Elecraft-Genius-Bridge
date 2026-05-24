# Production Baseline Freeze

`config.aethersdr-production.yaml` is the frozen known-good AetherSDR
production baseline.

Do not change this profile while working on SmartSDR experiments, meter
publication, interlock experiments, or PGXL/TGXL lab profiles.

## Frozen Properties

- PGXL listener bind IP: `127.0.0.1`
- PGXL advertised IP: `127.0.0.1`
- TGXL direct TCP is enabled.
- PGXL direct TCP is enabled.
- Flex amplifier injection is enabled with the current AetherSDR-compatible
  profile.
- KAT500 frequency-follow is enabled.
- KPA500 band-follow is enabled.
- KAT500 Tune is enabled.
- KPA500 Standby is enabled.
- KPA500 Operate is disabled.
- SmartSDR VITA meter publishing remains disabled.
- SmartSDR runtime interlock experiments remain disabled.

## Latest Production Evidence

Latest passing evidence showed:

- `aethersdr-production-test` result: PASS.
- PGXL direct connect delay: about `138 ms`.
- PGXL and TGXL sessions: both present.
- Tune executions: `6`.
- KPA band-follow: enabled and working.
- KAT frequency-follow: enabled and working.
- Amplifier removed count: `0`.
- Amplifier handle churn: none.
- PGXL direct KPA state reflection:
  - `STANDBY -> OPERATE`: about `114 ms`.
  - `OPERATE -> STANDBY`: about `46 ms`.
  - `STANDBY -> OPERATE`: about `158 ms`.

## Remaining Non-Baseline Issue

AetherSDR AMP operate/standby command emission remains absent:

- `flex-control-commands.log` is empty.
- `pgxl-control-commands.log` is empty.
- `control-events.jsonl` contains external KPA state changes and TGXL autotune
  events, but no `amplifier set ... operate=...` command and no direct PGXL
  operate/standby command.

EGB can map and execute KPA500 Standby if a command arrives. KPA500 Operate
remains RF-risk gated. The remaining work is client-side command emission or
Flex-side operability semantics, not the production PGXL/TGXL/KPA/KAT baseline.

## Required Regression Command

```powershell
.\target\release\egb.exe aethersdr-production-test --config .\config.aethersdr-production.yaml --duration-minutes 3
```


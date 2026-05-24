# AetherSDR Production Baseline

Use this profile for same-PC AetherSDR operation:

```powershell
.\target\release\egb.exe baseline-regression-test --config .\config.aethersdr-production.yaml --duration-minutes 3
```

The production profile intentionally keeps the known-good AetherSDR path separate from SmartSDR experiments.

## Required Network Shape

- `server.bind_ip: 127.0.0.1`
- `flex_injection.force_advertised_pgxl_ip: 127.0.0.1`

AetherSDR auto-opens PGXL TCP using the advertised Flex amplifier IP. If EGB is bound to loopback but advertises the Windows LAN IP, AetherSDR may wait for the slow manual/retry path instead of connecting immediately.

## Enabled Behaviour

- PGXL direct enabled.
- TGXL direct enabled.
- Flex amplifier injection enabled.
- `aethersdr_force_direct` retained for the current proven AetherSDR profile.
- KAT500 frequency-follow enabled.
- KPA500 band-follow enabled.
- KAT500 Tune enabled.
- KPA500 Standby enabled.
- KPA500 Operate disabled.
- VITA meter publishing disabled.
- Runtime interlock loop disabled.

## Regression Criteria

`baseline-regression-test` fails if:

- PGXL direct accept delay exceeds 2 seconds.
- PGXL session is missing.
- TGXL session is missing.
- KPA band-follow is disabled.
- KAT frequency-follow is disabled.
- Flex reports amplifier removal.
- runtime config hashes disagree.

SmartSDR meter/interlock tests must use their own profile and must not replace this AetherSDR baseline.

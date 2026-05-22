# Live Test Profiles

Use these profiles deliberately. The names describe what the bridge is allowed to do to real hardware.

## Monitor / Dry-Run Known-Good

Config:

```powershell
.\config.aethersdr-known-good.yaml
```

Purpose:
- Validate KPA500/KAT500 polling.
- Validate Flex amplifier registration.
- Validate PGXL/TGXL direct socket stability.
- Confirm AetherSDR sends commands.

Control behaviour:
- Real KAT500 Tune is blocked.
- Real KPA500 Standby is blocked.
- Real KPA500 Operate is blocked.

Expected blocked Tune message:

```text
Autotune received but blocked because this config is monitor/dry-run. Use config.aethersdr-real-operational.yaml for live tune testing.
```

## Real Operational Tune/Standby

Config:

```powershell
.\config.aethersdr-real-operational.yaml
```

Purpose:
- Execute AetherSDR Tune against KAT500.
- Execute KPA500 Standby when requested.
- Keep KPA500 Operate disabled.

Enabled real commands:
- KAT500 Tune: `T;`
- KPA500 Standby: `^OS0;`

Still blocked:
- KPA500 Operate: `^OS1;`
- KPA500 clear fault: `^FLC;`
- KAT500 antenna and bypass changes.

## RF-Risk Operate

There is no default RF-risk profile yet. This must remain a deliberate later test.

Required future conditions:
- Dummy load or other safe RF path.
- Local operator at the station.
- `enable_kpa_operate: true`
- Typed confirmation: `"I understand"`
- Evidence bundle enabled.

## Recommended Commands

Monitor-only validation:

```powershell
.\target\release\egb.exe evidence-test --config .\config.aethersdr-known-good.yaml --duration-minutes 5
```

Real Tune/Standby validation:

```powershell
.\target\release\egb.exe evidence-test --config .\config.aethersdr-real-operational.yaml --duration-minutes 5
```


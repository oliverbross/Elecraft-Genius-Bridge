# Live Test Profiles

Use these profiles deliberately. The names describe what the bridge is allowed to do to real hardware.

## Monitor / Dry-Run Known-Good

Config:

```powershell
.\config.aethersdr-known-good.yaml
```

Locked regression equivalent:

```powershell
.\config.aethersdr-last-known-good-operational.yaml
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
Autotune received but blocked because this config is monitor/dry-run. Use config.aethersdr-compatible-operational.yaml for live tune testing.
```

## AetherSDR Compatible Tune/Standby

Config:

```powershell
.\config.aethersdr-compatible-operational.yaml
```

Locked last-known-good real-control equivalent:

```powershell
.\config.aethersdr-last-known-good-real-controls.yaml
```

Purpose:
- Execute AetherSDR Tune against KAT500.
- Execute KPA500 Standby when requested.
- Keep KPA500 Operate disabled.
- Add the AetherSDR direct-connect readiness fields needed to trigger PGXL/TGXL direct TCP.

Enabled real commands:
- KAT500 Tune: `T;`
- KPA500 Standby: `^OS0;`

Still blocked:
- KPA500 Operate: `^OS1;`
- KPA500 clear fault: `^FLC;`
- KAT500 antenna and bypass changes.

Flex amplifier create line:

```text
amplifier create ip=192.168.0.189 port=9008 model=PowerGeniusXL serial_num=EGB-KPA500 ant=ANT1:PORTA,ANT2:PORTB state=<live-kpa-state>
```

The current compatibility profile uses `amplifier_status_profile: aethersdr_minimal`. The locked last-known-good profile uses `aethersdr_force_direct` because that is the most recent evidence-backed path where PGXL and TGXL both connected. Phase 49 safeguards still apply: KPA/KAT preflight must pass, the advertised IP must be reachable from the radio/client path, and `pgxl_connect_assist` remains off.

## Strict Official PGXL Audit

Config:

```powershell
.\config.aethersdr-real-operational.yaml
```

Purpose:
- Validate the official minimal Flex amplifier create command.
- Compare strict Flex/SmartSDR behavior against AetherSDR compatibility behavior.

Flex amplifier create line:

```text
amplifier create ip=192.168.0.189 port=9008 model=PowerGeniusXL serial_num=EGB-KPA500 ant=ANT1:PORTA,ANT2:PORTB
```

This profile may not trigger AetherSDR direct PGXL/TGXL sockets. It is retained for protocol audit work.

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
.\target\release\egb.exe connection-regression-test --config .\config.aethersdr-last-known-good-operational.yaml --duration-minutes 5
```

Real Tune/Standby validation:

```powershell
.\target\release\egb.exe evidence-test --config .\config.aethersdr-last-known-good-real-controls.yaml --duration-minutes 5
```

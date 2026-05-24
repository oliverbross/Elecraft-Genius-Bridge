# SmartSDR Experimental Profile

Use this profile only for SmartSDR PGXL meter/interlock experiments:

```powershell
.\target\release\egb.exe flex-runtime-test --config .\config.smartsdr-pgxl-meter-test.yaml --duration-minutes 5
```

## Important Difference From AetherSDR Production

The SmartSDR test profile binds and advertises the Windows LAN IP:

- `server.bind_ip: 192.168.0.189`
- `flex_injection.force_advertised_pgxl_ip: 192.168.0.189`

This avoids the invalid loopback/LAN mismatch that caused delayed PGXL opens in AetherSDR. Do not use this profile for same-PC AetherSDR baseline testing.

## Experimental Features

Enabled:

- `flex_injection.enable_runtime_interlock: true`
- `flex_injection.enable_vita_meter_publish: true`

Still protected:

- KPA500 Operate remains disabled.
- RF-risk remains disabled.
- Runtime interlock will answer `not_ready` unless KPA500 and RF-risk policy allow TX.

## Expected Output

Evidence should show:

- meter handles and meter publish counters,
- `interlock-runtime-events.jsonl` if Flex requests PTT,
- SmartSDR PGXL data behaviour,
- SmartSDR TGXL remains unsupported until a verified Flex tuner/accessory registration sequence is found.

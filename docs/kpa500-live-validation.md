# KPA500 Live Validation

Start with read-only plus safe standby:

```powershell
.\target-msvc\debug\egb.exe test-kpa-live --config .\config.flex-injection-readonly.yaml --allow-control
```

This performs firmware/status/power/SWR/temperature/voltage-current/fault reads and sends standby `^OS0;` only when `--allow-control` is present.

Operate testing is RF-risk and requires an explicit confirmation string:

```powershell
.\target-msvc\debug\egb.exe test-kpa-operate --config .\config.flex-injection-readonly.yaml --allow-rf-risk --confirm-rf-risk "I understand"
```

Operate workflow:

1. Force/verify standby.
2. Send `^OS1;`.
3. Verify `^OS1;`.
4. Immediately send `^OS0;`.
5. Verify `^OS0;`.

Every command creates an evidence ZIP under `diagnostics\runs`.

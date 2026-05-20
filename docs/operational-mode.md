# Operational Mode

Operational mode is the explicit path for using real KPA500/KAT500 controls from AetherSDR or the GUI.

Defaults remain safe:

- `dry_run: true` blocks all control-changing serial commands.
- `allow_rf_risk: false` blocks KPA500 operate and KAT500 tune.
- `operational.enable_real_controls: false` disables operational overrides.

To run real controls, load `config.aethersdr-operational.yaml`, review the COM ports and IP addresses, then enable only the actions you intend to test:

```yaml
operational:
  enable_real_controls: true
  enable_kat_tune: true
  enable_kat_bypass: false
  enable_kat_antenna: false
  enable_kpa_standby: true
  enable_kpa_operate: false
  enable_clear_fault: false
  persist_rf_risk: false
  confirm_real_hardware_control: "I understand"
```

Mapped commands:

| Action | Elecraft command | Gate |
| --- | --- | --- |
| KAT500 tune | `T;` | `enable_kat_tune=true` |
| KAT500 bypass | `BYPB;` / `BYPN;` | `enable_kat_bypass=true` |
| KAT500 antenna | `AN1;` / `AN2;` / `AN3;` | `enable_kat_antenna=true` |
| KPA500 standby | `^OS0;` | `enable_kpa_standby=true` |
| KPA500 operate | `^OS1;` | `enable_kpa_operate=true` |
| KPA500 clear fault | `^FLC;` | `enable_clear_fault=true` |

`enable_kpa_operate` and `enable_kat_tune` are RF-risk controls. Use them only during local testing with an appropriate load, antenna routing, and operator supervision.

The GUI exposes the same controls on the Operational page. RF-risk controls should be treated as session decisions; avoid persisting them unless the station is intentionally configured for unattended operation.

## Effective Policy

The raw device flags and the operational override are resolved into one effective runtime policy. `/status` exposes this as `effective_controls`, and the GUI Operational page shows both the raw flags and the resolved result.

This means `kat500.dry_run: true` can remain in the profile while `operational.enable_kat_tune: true` and the confirmation string allow only KAT500 `T;` to execute. The same pattern allows KPA500 standby `^OS0;` without enabling KPA500 operate `^OS1;`.

If Flex injection is degraded, EGB can still show PGXL/TGXL direct sockets as connected, but AetherSDR/SmartSDR applets that depend on Flex amplifier registration may be missing. Treat operational readiness as all of:

- hardware polling healthy
- PGXL/TGXL direct sockets connected
- Flex injection connected when AMP applets are expected
- effective control policy allows the requested action

# Controls Safety

Phase 21 adds a Controls page but keeps risky controls guarded.

## Runtime Control Flags

The GUI has session-only flags:

- Enable KPA safe controls
- Enable KPA RF-risk controls
- Enable KAT safe controls
- Enable KAT tune/RF-risk controls
- Enable KAT antenna switching
- Enable destructive/advanced actions

Defaults are all off.

## KPA500

Shown command paths:

```text
Standby: Flex amplifier set operate=0 -> KPA500 ^OS0;
Operate: Flex amplifier set operate=1 -> KPA500 ^OS1;
Clear fault: ^FLC;
```

Operate requires:

- GUI RF-risk control enabled
- `kpa500.allow_rf_risk: true`
- current session acknowledgement

The dedicated `test-kpa-operate` CLI path still immediately returns to standby.

## KAT500

Shown command paths:

```text
Antenna: AN1; / AN2; / AN3;
Tune: TGXL autotune -> KAT500 tune path
Bypass: BYP control path
```

Tune, bypass, and antenna switching remain disabled unless the corresponding runtime flags are enabled. Tune remains a transmit/RF-risk action.

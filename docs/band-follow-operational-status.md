# Band Follow Operational Status

Phase 63 evidence shows the band-follow path is now operational enough for controlled AetherSDR use.

## KAT500

KAT500 frequency-follow is operational.

- EGB receives Flex/AetherSDR transmit frequency context.
- EGB sends documented KAT500 frequency context, for example `F 24930;`.
- Latest evidence confirmed exact echo: requested `F 24930;`, confirmed `F 24930;`, `confirmation_match=true`.
- KAT500 Tune continues to send `T;` after the current Flex frequency is applied.

## KPA500

KPA500 band-follow is operational, but it is band-only.

- EGB maps Flex band to KPA500 `^BNnn;`.
- Latest evidence shows `follow_flex_band=true`, sent count `6`, and last wire `^BN08;`.
- Phase 63 adds exact `^BNnn;` confirmation so stale previous `^BN` responses are ignored the same way stale KAT500 `F` responses are ignored.

KPA500 has no verified direct frequency command in the command set EGB is using. EGB can follow the band with `^BNnn;`; KAT500 is the device that receives exact transmit frequency with `F <kHz>;`.

## Test Command

```powershell
.\target\release\egb.exe band-follow-test --config .\config.aethersdr-kpa-band-follow-test.yaml --duration-minutes 5
```

`band-follow-test` writes the same run evidence bundle as the other operational tests and prints the final ZIP path, for example:

```text
diagnostics\runs\YYYYMMDD-HHMMSS-band-follow-test.zip
```

The evidence bundle includes:

- `kat500-frequency-follow.log`
- `kpa500-band-follow-confirmation.log`
- `band-follow-summary.md`
- `status-start.json`
- `status-end.json`
- `flex-rx.log`
- `flex-tx.log`
- `kpa500-serial.log`
- `kat500-serial.log`
- `pgxl-protocol.log`
- `tgxl-protocol.log`

## KPA500 State Reflection

When KPA500 polling observes a real `^OS`, `^TM`, `^VI`, `^FL`, or `^WS` change, EGB updates the shared PGXL telemetry immediately and requests a Flex amplifier reannounce outside the normal refresh interval. External Standby/Operate changes made in KPA500 Remote should now reach AetherSDR on the next KPA poll plus the short requested-reannounce tick, normally within 1-2 seconds when serial polling is healthy.

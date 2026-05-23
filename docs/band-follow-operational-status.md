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

The evidence bundle includes:

- `kat500-frequency-follow.log`
- `kpa500-band-follow.log`
- `kpa500-band-follow-confirmation.log`
- `band-follow-summary.md`

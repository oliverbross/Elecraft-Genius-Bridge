# KPA500 Band Follow Test

EGB includes an explicit test-only KPA500 band-follow profile:

```powershell
.\target\release\egb.exe evidence-test --config .\config.aethersdr-kpa-band-follow-test.yaml --duration-minutes 5
```

The profile enables:

```yaml
kpa500:
  follow_flex_band: true
```

Behaviour:

- Flex TX slice band changes are mapped to KPA500 band numbers.
- EGB sends `^BNnn;` only when the derived band changes.
- EGB does not send KPA500 `^OS1;`.
- KPA500 operate remains disabled unless a separate RF-risk profile explicitly enables it.

Evidence fields:

- `/status.radio_context.last_kpa500_follow_band`
- `/status.radio_context.last_kpa500_follow_wire`
- `/status.radio_context.last_kpa500_follow_result`
- `/status.radio_context.kpa500_follow_sent_count`
- `/status.radio_context.kpa500_follow_skipped_count`

The test report should include:

- Flex band,
- KPA500 `^BNnn;` wire command,
- KPA500 response,
- AetherSDR PGXL state after the band change.

If the KPA500 rejects or ignores `^BNnn;`, disable `follow_flex_band` and keep only PGXL/Flex-side band context updates.

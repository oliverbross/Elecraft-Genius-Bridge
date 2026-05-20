# Amplifier Operate Lab

`egb amplifier-operate-lab` tests whether the Flex radio owns and rewrites the injected PGXL amplifier operate state.

The command is lab-only. It does not send `^OS1;` to the KPA500 and does not enable RF output. It only sends this Flex API command after the injected amplifier handle is observed:

```text
amplifier set <handle> operate=1
```

Run:

```powershell
.\target-msvc\debug\egb.exe amplifier-operate-lab --config .\config.aethersdr-known-good.yaml --duration-seconds 60
```

The evidence bundle includes:

- `amplifier-operate-lab.md`
- `flex-amplifier-operate-sequence.log`
- `amplifier-state-rewrite-analysis.md`
- `amplifier-advertisements.jsonl`
- `radio-stripped-amplifier-fields.md`
- `status-start.json`
- `status-end.json`

Interpretation:

- If the Flex API accepts `amplifier set <handle> operate=1` and subsequent `S...|amplifier` status changes to `state=OPERATE`, the Flex-side operate state can be manipulated without touching the KPA500.
- If the command is accepted but status remains `state=STANDBY`, the radio is rewriting or refusing effective amplifier operate state.
- If AetherSDR opens TCP 9008 after the status changes to `OPERATE`, `STANDBY` was the likely direct-connect suppressor.
- If AetherSDR still does not open TCP 9008 after `OPERATE`, the remaining trigger is probably outside the state field: binary/source mismatch, manual peripheral settings, or a radio-side accessory detail not yet replicated.

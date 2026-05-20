# PGXL Direct Trigger Matrix

`egb pgxl-direct-trigger-matrix` is a lab-only command for the remaining AetherSDR PGXL pairing problem: AetherSDR sees the injected amplifier but does not always open TCP 9008.

The command forces only the advertised Flex amplifier state to `OPERATE`. It does not send `^OS1;` or any other KPA500 control command. The real KPA500 state remains unchanged.

Run:

```powershell
.\target-msvc\debug\egb.exe pgxl-direct-trigger-matrix --config .\config.aethersdr-known-good.yaml --duration-seconds 60
```

The evidence bundle includes:

- `pgxl-trigger-matrix.md`
- `radio-stripped-amplifier-fields.md`
- `aethersdr-amp-parser-notes.md`
- `amplifier-advertisements.jsonl`
- `status-start.json`
- `status-end.json`

Interpretation:

- If `pgxl_session_started_count` becomes greater than zero, AetherSDR accepted the lab advertisement and opened TCP 9008.
- If the radio-side observed status line is missing fields that EGB emitted, the Flex radio is stripping unsupported fields before AetherSDR sees them.
- If the observed line contains `model=PowerGeniusXL` and a non-empty `ip` but AetherSDR still does not open TCP 9008, the installed AetherSDR binary may differ from the inspected source or a UI/peripheral setting may be suppressing auto-connect.

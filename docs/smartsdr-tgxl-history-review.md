# SmartSDR TGXL History Review

## Evidence Found

The repository contains one early validation note, `docs/validation/smartsdr-tuner-success.md`, stating that SmartSDR once saw an injected tuner. That note predates the current PGXL/TGXL direct operational path and does not identify a commit, config, Flex command sequence, or reproducible evidence bundle with a current `tuner_handle`.

Recent evidence bundles consistently show:

- `tuner_appeared_count=0`
- `tuner_handle=null`
- no verified Flex-side TGXL/tuner registration command
- AetherSDR direct TGXL TCP works independently of SmartSDR

## Diff Result

No current code path registers a documented external TGXL/tuner object with the Flex radio. EGB supports direct TGXL TCP and Flex slice tracking, but that is not the same discovery path SmartSDR appears to require for a visible tuner widget.

## Current Conclusion

SmartSDR TGXL visibility cannot be restored by changing the direct TGXL TCP emulator alone. The missing artifact is a verified Flex-side tuner/accessory registration sequence, either from official documentation or from a real TGXL-to-Flex capture.

## Next Evidence Needed

Capture a real TGXL paired to a Flex radio and record:

- Flex TCP TX/RX from the TGXL client.
- Any `tuner`, `atu`, `interlock`, `meter`, or accessory registration commands.
- Status lines SmartSDR receives when the TGXL becomes visible.

Until then, EGB should report SmartSDR TGXL as unsupported while preserving AetherSDR direct TGXL support.

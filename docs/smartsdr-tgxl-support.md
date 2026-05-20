# SmartSDR TGXL Support

Current support is split by client path:

- AetherSDR can use the direct TGXL TCP endpoint on port `9010`.
- SmartSDR does not use the direct TGXL endpoint alone for stable tuner visibility.
- SmartSDR tuner visibility requires a Flex-side tuner/accessory registration path.

EGB currently captures Flex RX/TX, tuner presence counters, and SmartSDR visibility evidence during `full-operational-test`, but a public, verified Flex API command sequence for registering an external Tuner Genius XL equivalent has not been confirmed.

Until that command sequence is verified, SmartSDR TGXL visibility remains documented as unsupported. Do not treat successful AetherSDR TGXL direct operation as proof that SmartSDR can see the tuner.

The evidence files to inspect are:

- `flex-rx.log`
- `flex-tx.log`
- `smartsdr-visibility-analysis.md`
- `applet-visibility-paths.md`
- `disconnect-events.jsonl`

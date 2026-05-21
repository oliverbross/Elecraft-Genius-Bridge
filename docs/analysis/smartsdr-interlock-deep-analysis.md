# SmartSDR Interlock Deep Analysis

SmartSDR TGXL support remains experimental. AetherSDR can use EGB's direct TGXL TCP endpoint, but SmartSDR appears to depend on Flex-side tuner/accessory registration and interlock semantics that are not fully verified.

Observed risk areas:

- SmartSDR may not use direct TGXL TCP discovery the same way AetherSDR does.
- SmartSDR Tune can report an interlock problem if the injected amplifier/tuner interlock does not match the active TX antenna topology.
- The currently verified Flex-side path is strongest for PGXL amplifier visibility, not full TGXL registration.
- Missing or mismatched `valid_antennas`, amplifier interlock type, tuner object lifetime, or radio-native tuner state may prevent SmartSDR from treating the virtual TGXL as safe to use.

EGB support bundles now include enough evidence to separate these cases:

- Flex RX/TX logs
- interlock handle and valid antenna map
- SmartSDR tuner appeared/disappeared counters
- TGXL direct client sessions
- control command visibility

Until a public or captured Flex command sequence for external TGXL registration is verified, SmartSDR TGXL should be reported as `EXPERIMENTAL` in operational readiness. AetherSDR TGXL direct operation remains the supported tuner path.

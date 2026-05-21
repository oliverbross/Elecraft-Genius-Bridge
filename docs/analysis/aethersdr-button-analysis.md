# AetherSDR Button Analysis

Current operational evidence shows AetherSDR can display both applets and can maintain PGXL/TGXL direct sessions. TGXL Tune commands have been observed as direct `autotune` commands, proving the tuner button can emit traffic when the applet is enabled and the direct TGXL session is active.

The remaining AMP-control uncertainty is whether AetherSDR emits amplifier controls through the Flex radio API, direct PGXL TCP, or both for the tested binary. EGB records all three paths:

- Flex API: `controls.last_flex_amp_set_command`
- PGXL direct: `controls.last_pgxl_control_command`
- TGXL direct: `controls.last_tgxl_control_command`

If the GUI shows no command after a button press, the button is disabled or gated inside AetherSDR before any packet reaches EGB. Likely gates are amplifier operate readiness, Flex amplifier state, PGXL direct connected state, or layout/control availability in the installed binary.

Use the GUI Support page command simulator to validate EGB mapping independently of AetherSDR:

- Simulate Tune -> `KAT500 T;`
- Simulate Standby -> `KPA500 ^OS0;`
- Simulate Operate / Flex amplifier set -> `KPA500 ^OS1;`

The simulator does not replace live validation. It confirms that configuration and safety policy would allow or block the mapped Elecraft command before testing from AetherSDR.

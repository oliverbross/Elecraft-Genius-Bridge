# AetherSDR AMP Button Command Path

Observed live runs show PGXL direct TCP connects and polls, but AMP Standby/Operate button presses do not reach EGB:

- no Flex `amplifier set <handle> operate=<0|1>` observed
- no PGXL direct `operate`/`standby` command observed
- `/status.controls.last_flex_amp_set_command` remains `null`
- `/status.controls.last_pgxl_control_command` remains `null`

This means EGB cannot execute `^OS0;` or `^OS1;` because no client command arrives.

Likely AetherSDR-side gating inputs to keep comparing:

- Flex amplifier state and handle
- PGXL direct status state
- AMP meter handles and live meter values
- Flex interlock state and `tx_allowed`
- whether the button is a status indicator until AetherSDR sees an operate-capable radio-side amplifier object

EGB's internal mappings are:

- Flex `amplifier set <handle> operate=0` -> KPA500 `^OS0;` when safe standby is enabled
- Flex `amplifier set <handle> operate=1` -> KPA500 `^OS1;` only when RF-risk mode is enabled
- PGXL direct `standby` -> KPA500 `^OS0;` when safe standby is enabled
- PGXL direct `operate` -> KPA500 `^OS1;` only when RF-risk mode is enabled

Use `egb operational-gap-test` plus the simulator commands in `docs/aethersdr-amp-button-gating.md` to separate EGB mapping from AetherSDR UI enablement.

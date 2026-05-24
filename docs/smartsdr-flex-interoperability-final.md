# SmartSDR Flex Interoperability Final Notes

## PGXL Object

SmartSDR can see the virtual PGXL object when Flex amplifier registration is
healthy. That does not prove full SmartSDR PGXL interoperability.

Current limitations:

- AMP meter handles are created.
- No verified VITA-49 AMP meter publication is implemented.
- SmartSDR may therefore show the object without useful live power, return loss,
  current, or temperature data.

The available public evidence points to AMP meter values being VITA-49 UDP data
sent to radio port `4991`, not a simple TCP `meter set` command.

## AMP Standby/Operate

EGB now accepts explicit Flex `operate=0|1` status and external-client
`state=STANDBY|OPERATE` amplifier status as possible client control requests,
while ignoring its own reflected status. Operate is still RF-risk gated.

If SmartSDR changes its local UI but no Flex amplifier control/status reaches
EGB, the remaining blocker is client/radio-side control semantics rather than
Elecraft serial control.

## TGXL / Tuner

SmartSDR TGXL support remains unsupported until a verified Flex-side external
tuner registration sequence is found. AetherSDR uses direct TGXL TCP on `9010`;
SmartSDR does not use that direct socket path.

Historical notes mention a previous “injected tuner” visibility observation, but
the repo only contains a text fixture for that observation, not a reproducible
Flex registration implementation. The current implementation should not claim
SmartSDR TGXL support without a real TGXL capture or official tuner API.


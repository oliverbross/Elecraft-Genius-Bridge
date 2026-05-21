# Flex Object Ownership

This note separates what Flex owns from what EGB owns. The distinction matters because previous instability looked like EGB and the radio were fighting over amplifier state.

## Flex Owns

- The amplifier object handle after `amplifier create` is accepted.
- The radio-side amplifier status broadcast.
- The interlock object state and whether a given TX antenna is valid.
- Meter handle allocation.
- Client handle allocation and command responses.
- Removal of objects when the Flex session ends, keepalive fails, or a command/registration is rejected.

## EGB Owns

- The TCP 9008 PGXL direct server.
- The TCP 9010 TGXL direct server.
- The initial Flex amplifier registration request.
- Meter/interlock create requests.
- KPA500/KAT500 serial polling.
- KPA500/KAT500 command safety gates.
- Desired state mapping from PGXL/TGXL/Flex commands into Elecraft serial commands.
- Evidence logging and lifecycle diagnostics.

## Rules

- Create the amplifier once per Flex TCP session.
- Do not recreate objects because telemetry changes.
- Do not recreate objects because PGXL direct TCP reconnects.
- Do not force `operate=1` as a general lifecycle mechanism.
- Treat `amplifier <handle> removed` as a high-signal event and capture the preceding state.
- Keep KPA500 RF state authoritative for PGXL direct status.

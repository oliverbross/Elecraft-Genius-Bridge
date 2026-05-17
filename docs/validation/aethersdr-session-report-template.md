# AetherSDR Session Report Template

Use one report per validation session.

## Environment

- Date:
- Tester:
- AetherSDR version:
- AetherSDR commit, if known:
- macOS version:
- Bridge commit:
- Bridge OS:
- Network path: local / LAN / tunnel / other

## Config Summary

- Config file:
- Bind IP:
- PGXL port:
- TGXL port:
- KPA500 mode: mock / hardware dry-run / hardware control
- KAT500 mode: mock / hardware dry-run / hardware control
- `protocol_trace`: true / false
- Protocol transcript directory:
- Serial transcript directory:

## PGXL Result

- Manual IP connect result:
- Connected state shown in AetherSDR:
- First greeting observed:
- `info` response accepted:
- `status` response accepted:
- Polling interval observed:
- Operate/standby exposed in UI:
- Operate/standby command observed:
- Screenshot path:
- Protocol transcript path:

## TGXL Result

- Manual IP connect result:
- Connected state shown in AetherSDR:
- First greeting observed:
- `info` response accepted:
- `status` response accepted:
- Polling interval observed:
- Autotune exposed:
- Antenna buttons exposed:
- Manual tune controls exposed:
- Screenshot path:
- Protocol transcript path:

## Mismatches

List every unexpected inbound command, rejected response, disconnect, UI mismatch, or parse warning.

| Time | Device | Transcript line | Expected | Actual | Next fix |
| --- | --- | --- | --- | --- | --- |
| | | | | | |

## UI Behaviour

- PGXL values displayed:
- TGXL values displayed:
- Disconnect/reconnect behaviour:
- Any visible errors:

## Next Fixes Needed

- Protocol docs to update:
- Golden tests to add or change:
- Emulator changes to make after docs/tests:
- Hardware safety notes:

# Elecraft Command Mapping

Status: unverified placeholders.

The current serial drivers compile and provide reconnect/poll scaffolding, but the actual Elecraft command strings are not hardware-verified in this repository.

## KPA500

| Intent | Placeholder command | Verification |
| --- | --- | --- |
| Poll status | `ST;` | Unverified |
| Operate | `OP1;` | Unverified |
| Standby | `OP0;` | Unverified |
| Clear fault | `FC;` | Unverified |

## KAT500

| Intent | Placeholder command | Verification |
| --- | --- | --- |
| Poll status | `ST;` | Unverified |
| Autotune | `T;` | Unverified |
| Bypass on | `BP1;` | Unverified |
| Bypass off | `BP0;` | Unverified |
| Antenna select | `AN<n>;` | Unverified |

## Rule For Future Work

Do not silently change these mappings based on memory or guesswork. Add either:

- Official Elecraft command reference excerpts in `docs/`.
- Hardware transcript captures.
- Test notes with firmware versions.

Then update the driver parser and this table together.


# Elecraft Command Mapping

Status: unverified placeholders with safety classifications.

The current serial drivers compile and provide reconnect/poll scaffolding, but the actual Elecraft command strings are not hardware-verified in this repository.

## KPA500

| Intent | Placeholder command | Safety | Verification |
| --- | --- | --- | --- |
| Poll status | `ST;` | `read_only` | Unverified |
| Read version | `RV;` | `read_only` | Unverified |
| Operate | `OP1;` | `rf_risk` | Unverified |
| Standby | `OP0;` | `state_change_safe` | Unverified |
| Clear fault | `FC;` | `destructive_or_unknown` | Unverified |

## KAT500

| Intent | Placeholder command | Safety | Verification |
| --- | --- | --- | --- |
| Poll status | `ST;` | `read_only` | Unverified |
| Read version | `RV;` | `read_only` | Unverified |
| Autotune | `T;` | `rf_risk` | Unverified |
| Bypass on | `BP1;` | `state_change_safe` | Unverified |
| Bypass off | `BP0;` | `state_change_safe` | Unverified |
| Antenna select | `AN<n>;` | `state_change_safe` | Unverified |
| Manual relay move | `UNVERIFIED_MANUAL_TUNE;` | `destructive_or_unknown` | Unverified |

## Dry-Run Behaviour

With `dry_run: true`, the serial drivers permit only `read_only` commands. They block `state_change_safe`, `rf_risk`, and `destructive_or_unknown` commands and log the blocked command label, wire string, and safety class.

The `egb test-kpa` and `egb test-kat` commands default to read-only tests. `--allow-control` is required before state-changing control tests are attempted. `--allow-rf-risk` is required before RF-risk tests are attempted. `destructive_or_unknown` commands are not sent by these test commands.

## Rule For Future Work

Do not silently change these mappings based on memory or guesswork. Add either:

- Official Elecraft command reference excerpts in `docs/`.
- Hardware transcript captures.
- Test notes with firmware versions.

Then update the driver parser and this table together.

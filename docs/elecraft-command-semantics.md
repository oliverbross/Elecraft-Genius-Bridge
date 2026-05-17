# Elecraft Command Semantics

This registry documents response expectations and verification requirements for active Elecraft commands. Do not change control semantics without updating tests and hardware transcripts.

## Result States

| State | Meaning |
| --- | --- |
| `acknowledged` | Command returned a direct response/ACK. |
| `verified` | Command state was proven by a follow-up query. |
| `sent_no_ack` | Command was written successfully and no immediate ACK was expected. |
| `verify_failed` | Follow-up query returned, but did not prove the requested state. |
| `timeout` | Expected response or verification query timed out. |
| `parse_failed` | Verification response was present but could not be parsed. |

## KPA500

| Command | Purpose | Expects response | Requires post-verify | Verification method | Safety | Verified status | Transcript evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `^RVM;` | Read firmware | yes | no | n/a | read_only | hardware verified | `^RVM01.54;` |
| `^SN;` | Read serial number | yes | no | n/a | read_only | pending transcript | none |
| `^OS;` | Read operate/standby | yes | no | n/a | read_only | hardware verified | `^OS0;` |
| `^WS;` | Read power/SWR | yes | no | n/a | read_only | hardware verified | `^WS000 000;` |
| `^TM;` | Read temperature | yes | no | n/a | read_only | hardware verified | `^TM030;` |
| `^VI;` | Read PA voltage/current | yes | no | n/a | read_only | hardware verified | `^VI689 000;`, `^VI690 000;` |
| `^FL;` | Read fault | yes | no | n/a | read_only | hardware verified | `^FL00;` |
| `^OS0;` | Set standby | no | yes | wait `control.verify_delay_ms`, send `^OS;`, expect `^OS0;` | state_change_safe | hardware behaviour observed as no-ACK; post-verify implemented | `tests/fixtures/kpa500-standby-noack-verify-com21.txt` |
| `^OS1;` | Set operate | no | yes | not enabled in safe-control phase | rf_risk | not control-validated | none |
| `^FLC;` | Clear fault | no | yes | not enabled | destructive_or_unknown | not control-validated | none |

## KAT500

| Command | Purpose | Expects response | Requires post-verify | Verification method | Safety | Verified status | Transcript evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `;` | Wake/null | yes | no | n/a | read_only | hardware verified | `;` |
| `RV;` | Read firmware | yes | no | n/a | read_only | hardware verified | `RV02.16;` |
| `SN;` | Read serial number | yes | no | n/a | read_only | hardware verified | `SN 3867;` |
| `AN;` | Read antenna | yes | no | n/a | read_only | hardware verified | `AN2;` |
| `BYP;` | Read bypass | yes | no | n/a | read_only | hardware verified | `BYPN;` |
| `MD;` | Read mode | yes | no | n/a | read_only | hardware verified | `MDA;` |
| `TP;` | Read tune power/status | yes | no | n/a | read_only | hardware verified | `TP0;` |
| `FLT;` | Read fault | yes | no | n/a | read_only | hardware verified | `FLT0;` |
| `VSWR;` | Read SWR | yes | no | n/a | read_only | hardware verified | `VSWR 1.11;` |
| `VFWD;` | Read forward power/ADC | yes | no | n/a | read_only | hardware verified | `VFWD 0;` |
| `BYPB;` / `BYPN;` | Change bypass | unknown | yes | intentionally blocked in Phase 13 | state_change_safe | not control-validated | none |
| `AN<n>;` | Change antenna | unknown | yes | intentionally blocked in Phase 13 | state_change_safe | not control-validated | none |
| `T;` | Tune | unknown | yes | intentionally blocked in Phase 13 | rf_risk | not control-validated | none |

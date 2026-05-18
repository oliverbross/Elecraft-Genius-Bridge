# Elecraft Command Mapping

Status: KPA500 mappings use the caret-prefixed Programmer Reference commands provided during Phase 8. KAT500 mappings use the Elecraft KAT500 Serial Command Reference v2.0 where explicitly identified.

The previous active assumptions `ST;`, `RV;`, `OP1;`, `OP0;`, and `FC;` for KPA500 were wrong or unverified. They are no longer used by the KPA500 driver.

Primary references:

- KPA500 Programmer Reference command list supplied in Phase 8.
- Elecraft KAT500 Automatic Antenna Tuner Serial Command Reference v2.0: `https://ftp.elecraft.com/KAT500/Manuals%20Downloads/archive/KAT500%20Automatic%20Antenna%20Tuner%20Serial%20Command%20Reference%20v2.0.pdf`

## KPA500

| Intent | Wire command | Safety | Verification |
| --- | --- | --- | --- |
| Read firmware | `^RVM;` | `read_only` | Hardware verified on COM21 at 38400: `^RVM01.54;` |
| Read serial number | `^SN;` | `read_only` | Programmer Reference |
| Read operate/standby | `^OS;` | `read_only` | Hardware verified on COM21: `^OS0;` |
| Read power/SWR | `^WS;` | `read_only` | Hardware verified on COM21: `^WS000 000;`, live RF sample `^WS030 011;` |
| Read temperature | `^TM;` | `read_only` | Hardware verified on COM21: `^TM030;` |
| Read PA volts/current | `^VI;` | `read_only` | Hardware verified on COM21: `^VI689 000;` / `^VI690 000;` |
| Read fault | `^FL;` | `read_only` | Hardware verified on COM21: `^FL00;` |
| Standby | `^OS0;` | `state_change_safe` | Programmer Reference, still gated |
| Operate | `^OS1;` | `rf_risk` | Programmer Reference, still gated |
| Clear fault | `^FLC;` | `destructive_or_unknown` | Programmer Reference, not sent by test CLI |

`^WSppp sss;` is parsed as forward power watts plus SWR encoded in tenths. `^WS030 011;` is treated as `30 W` forward power and SWR `1.1`. `sss=000` is treated as no-RF/no-SWR-measurement and mapped to SWR `1.0` for bridge state.

Confirmed hardware baseline:

- KPA500 on `COM21`
- baud `38400`
- no CR/LF terminator
- probe `^RVM;`
- response `^RVM01.54;`

`^VIvvv iii;` reports PA voltage/current in tenths. `^VI689 000;` means `68.9 V` and `0.0 A`; it is not AC mains voltage and is not published as PGXL `vac`.

## KAT500

The KAT500 uses ordinary ASCII commands and may sleep. Wake discovery sends single semicolon null commands at roughly 100 ms intervals until semicolon responses are seen.

| Intent | Wire command | Safety | Verification |
| --- | --- | --- | --- |
| Wake/null probe | `;` | `read_only` | KAT500 Serial Command Reference |
| Read firmware | `RV;` | `read_only` | Hardware verified on COM8: `RV02.16;` |
| Read serial number | `SN;` | `read_only` | Hardware verified on COM8: `SN 3867;` |
| Read antenna | `AN;` | `read_only` | Hardware verified on COM8: `AN2;` |
| Read bypass relay | `BYP;` | `read_only` | Hardware verified on COM8: `BYPN;` |
| Read mode | `MD;` | `read_only` | Hardware verified on COM8: `MDA;` |
| Read tune progress | `TP;` | `read_only` | Hardware verified on COM8: `TP0;` |
| Read fault | `FLT;` | `read_only` | Hardware verified on COM8: `FLT0;` |
| Read VSWR | `VSWR;` | `read_only` | Hardware verified on COM8: `VSWR 1.11;` |
| Read forward ADC | `VFWD;` | `read_only` | Hardware verified on COM8: `VFWD 0;` |
| Bypass on | `BYPB;` | `state_change_safe` | KAT500 Serial Command Reference, still gated |
| Bypass off | `BYPN;` | `state_change_safe` | KAT500 Serial Command Reference, still gated |
| Antenna select | `AN<n>;` | `state_change_safe` | KAT500 Serial Command Reference, still gated |
| Full tune | `T;` | `rf_risk` | KAT500 Serial Command Reference, still gated |
| Manual relay move | `UNVERIFIED_MANUAL_TUNE;` | `destructive_or_unknown` | Not active |

`ST;` is intentionally not used as a KAT500 status poll. The KAT500 reference defines `STbbt;` as SWR-threshold GET/SET, not generic status.

Confirmed hardware baseline:

- KAT500 on `COM8`
- baud `38400`, `19200`, and `9600` all returned valid command responses
- preferred baud remains configured `38400`
- terminator: none
- read-only fixture: `tests/fixtures/kat500-readonly-com8.txt`

## Dry-Run Behaviour

With `dry_run: true`, the serial drivers permit only `read_only` commands. They block `state_change_safe`, `rf_risk`, and `destructive_or_unknown` commands and log the blocked command label, wire string, and safety class.

The `egb test-kpa` and `egb test-kat` commands default to read-only tests. `--allow-control` is required before state-changing control tests are attempted. `--allow-rf-risk` is required before RF-risk tests are attempted. `destructive_or_unknown` commands are not sent by these test commands.

`egb test-kpa-operate --allow-rf-risk` is the explicit local-only RF-risk workflow. It forces standby, sends `^OS1;`, verifies with `^OS;`, immediately sends `^OS0;`, and verifies standby rollback.

Command ACK and verification semantics are tracked in `docs/elecraft-command-semantics.md`.

## Rule For Future Work

Do not silently change these mappings based on memory or guesswork. Add either:

- Official Elecraft command reference excerpts in `docs/`.
- Hardware transcript captures.
- Test notes with firmware versions.

Then update the driver parser and this table together.

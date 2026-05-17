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
| Read operate/standby | `^OS;` | `read_only` | Programmer Reference |
| Read power/SWR | `^WS;` | `read_only` | Programmer Reference |
| Read temperature | `^TM;` | `read_only` | Programmer Reference |
| Read volts/current | `^VI;` | `read_only` | Programmer Reference |
| Read fault | `^FL;` | `read_only` | Programmer Reference |
| Standby | `^OS0;` | `state_change_safe` | Programmer Reference, still gated |
| Operate | `^OS1;` | `rf_risk` | Programmer Reference, still gated |
| Clear fault | `^FLC;` | `destructive_or_unknown` | Programmer Reference, not sent by test CLI |

`^WSppp sss;` is parsed as forward power watts plus SWR encoded as hundredths. `sss=000` is treated as no-RF/no-SWR-measurement and mapped to SWR `1.0` for bridge state.

Confirmed hardware baseline:

- KPA500 on `COM21`
- baud `38400`
- no CR/LF terminator
- probe `^RVM;`
- response `^RVM01.54;`

## KAT500

The KAT500 uses ordinary ASCII commands and may sleep. Wake discovery sends single semicolon null commands at roughly 100 ms intervals until semicolon responses are seen.

| Intent | Wire command | Safety | Verification |
| --- | --- | --- | --- |
| Wake/null probe | `;` | `read_only` | KAT500 Serial Command Reference |
| Read firmware | `RV;` | `read_only` | KAT500 Serial Command Reference |
| Read serial number | `SN;` | `read_only` | KAT500 Serial Command Reference |
| Read antenna | `AN;` | `read_only` | KAT500 Serial Command Reference |
| Read bypass relay | `BYP;` | `read_only` | KAT500 Serial Command Reference |
| Read mode | `MD;` | `read_only` | KAT500 Serial Command Reference |
| Read tune progress | `TP;` | `read_only` | KAT500 Serial Command Reference |
| Read fault | `FLT;` | `read_only` | KAT500 Serial Command Reference |
| Read VSWR | `VSWR;` | `read_only` | KAT500 Serial Command Reference |
| Read forward ADC | `VFWD;` | `read_only` | KAT500 Serial Command Reference |
| Bypass on | `BYPB;` | `state_change_safe` | KAT500 Serial Command Reference, still gated |
| Bypass off | `BYPN;` | `state_change_safe` | KAT500 Serial Command Reference, still gated |
| Antenna select | `AN<n>;` | `state_change_safe` | KAT500 Serial Command Reference, still gated |
| Full tune | `T;` | `rf_risk` | KAT500 Serial Command Reference, still gated |
| Manual relay move | `UNVERIFIED_MANUAL_TUNE;` | `destructive_or_unknown` | Not active |

`ST;` is intentionally not used as a KAT500 status poll. The KAT500 reference defines `STbbt;` as SWR-threshold GET/SET, not generic status.

## Dry-Run Behaviour

With `dry_run: true`, the serial drivers permit only `read_only` commands. They block `state_change_safe`, `rf_risk`, and `destructive_or_unknown` commands and log the blocked command label, wire string, and safety class.

The `egb test-kpa` and `egb test-kat` commands default to read-only tests. `--allow-control` is required before state-changing control tests are attempted. `--allow-rf-risk` is required before RF-risk tests are attempted. `destructive_or_unknown` commands are not sent by these test commands.

## Rule For Future Work

Do not silently change these mappings based on memory or guesswork. Add either:

- Official Elecraft command reference excerpts in `docs/`.
- Hardware transcript captures.
- Test notes with firmware versions.

Then update the driver parser and this table together.

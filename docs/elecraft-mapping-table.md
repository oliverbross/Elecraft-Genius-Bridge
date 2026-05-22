# Elecraft Mapping Table

This table maps official PGXL/TGXL behavior onto verified Elecraft KPA500/KAT500 commands.

## TGXL to KAT500

| Official TGXL behavior | Elecraft command | Verification | Safety |
|---|---|---|---|
| `autotune` | `F <kHz>;` then `T;` | `F` is documented frequency context; `T;` is hardware-confirmed executable | RF-risk/tune gated |
| `bypass set=1` | `BYP;` | Command mapping present; validate before broad use | State-change safe, gated |
| `bypass set=0` | `BYPN;` | Command mapping present; validate before broad use | State-change safe, gated |
| `activate ant=N` | `AN<n>;` | Command mapping present; validate before broad use | State-change safe, gated |
| `operate set=0/1` | none | Virtual TGXL state only | No RF action |
| `tune relay=N move=+1/-1` | none | Unsupported until KAT500 relay equivalents are verified | Destructive/unknown |
| `status VSWR` | `VSWR;` | Verified | Read-only |
| `status VFWD` | `VFWD;` | Verified | Read-only |
| antenna status | `AN;` | Verified | Read-only |
| bypass status | `BYP;` read | Verified | Read-only |
| mode status | `MD;` | Verified | Read-only |
| tune state/power | `TP;` | Verified | Read-only |
| fault status | `FLT;` | Verified | Read-only |
| firmware | `RV;` | Verified | Read-only |
| serial | `SN;` | Verified | Read-only |

## PGXL/Flex to KPA500

| Official PGXL/Flex behavior | Elecraft command | Verification | Safety |
|---|---|---|---|
| Standby / operate=0 | `^OS0;` | Verified ack-less with post-verify `^OS;` | Safe control, gated |
| Operate / operate=1 | `^OS1;` | Command known, RF-risk gated | RF-risk |
| Clear fault | `^FLC;` | Mapped but intentionally dangerous | Destructive/advanced |
| Firmware | `^RVM;` | Verified: `^RVM01.54;` | Read-only |
| Serial | `^SN;` | Pending unless current transcript proves it | Read-only |
| Operate status | `^OS;` | Verified: `^OS0;` / `^OS1;` | Read-only |
| Forward/SWR | `^WS;` | Verified, including no-RF `^WS000 000;` and `^WS030 011;` | Read-only |
| Temperature | `^TM;` | Verified | Read-only |
| Voltage/current | `^VI;` | Verified; value is internal PA supply/current, not AC mains | Read-only |
| Fault | `^FL;` | Verified: `^FL00;` | Read-only |

## Known Limitations

- KAT500 band/frequency context uses documented `F <kHz>;` before tune. Raw relay forcing remains unsupported.
- PGXL `vac` is not populated from KPA500 `^VI` unless voltage semantics are validated against PGXL expectations.
- TGXL manual relay controls remain blocked because KAT500 relay equivalents are not verified.

# Real Hardware Results

This file records actual KPA500/KAT500 serial validation results from Oliver's station.

## 2026-05-17 Phase 11

### Read-Only Integration Status

Continuous read-only polling now works for both real devices while AetherSDR connects and polls the PGXL/TGXL endpoints.

Important UI note:

- Direct PGXL/TGXL sockets are connected and stable.
- AetherSDR applet window visibility remains a separate Flex radio model/presence problem.
- PGXL AMP visibility likely requires Flex radio-side `amplifier` status or an equivalent non-proxy configuration.
- TGXL TUN visibility may depend on the exact AetherSDR binary/source revision and layout state.

### KPA500 Read-Only Validation

Result: KPA500 read-only polling confirmed except serial number, which remains pending until `^SN;` is observed in a transcript.

Confirmed:

- Port: `COM21`
- Baud: `38400`
- Terminator: none
- Protocol: caret-prefixed KPA500 Programmer Reference command set

Observed responses:

```text
^RVM; -> ^RVM01.54;
^OS; -> ^OS0;
^WS; -> ^WS000 000;
^TM; -> ^TM030;
^VI; -> ^VI689 000; or ^VI690 000;
^FL; -> ^FL00;
```

Parsed state:

- firmware `01.54`
- operate `false`
- state `STANDBY`
- forward power `0`
- SWR `1.0` as safe no-RF state
- temperature `30.0 C`
- PA voltage `68.9 V` or `69.0 V`
- PA current `0.0 A`
- fault `0`

`^VI` is PA voltage/current in tenths, not AC mains voltage. EGB stores it as internal PA voltage/current and does not publish `68.9` as PGXL `vac`.

Regression fixtures:

```text
tests/fixtures/kpa500-rvm-com21.txt
tests/fixtures/kpa500-readonly-com21.txt
```

### KAT500 Read-Only Validation

Result: KAT500 read-only polling confirmed.

Confirmed:

- Port: `COM8`
- Terminator: none
- Baud: `38400`, `19200`, and `9600` all returned valid command responses
- Preferred configured baud: `38400`

Observed responses:

```text
; -> ;
RV; -> RV02.16;
SN; -> SN 3867;
AN; -> AN2;
BYP; -> BYPN;
MD; -> MDA;
TP; -> TP0;
FLT; -> FLT0;
VSWR; -> VSWR 1.11;
VFWD; -> VFWD 0;
```

Parsed state:

- firmware `02.16`
- serial `3867`
- antenna `2`
- bypass `false`
- mode `auto`
- tune power/status `0`
- fault `0`
- SWR `1.11`
- forward power `0`

Regression fixture:

```text
tests/fixtures/kat500-readonly-com8.txt
```

Next commands:

```powershell
.\target-msvc\debug\egb.exe test-kpa --config .\config.hardware-readonly.yaml
.\target-msvc\debug\egb.exe test-kat --config .\config.hardware-readonly.yaml
.\target-msvc\debug\egb.exe run --config .\config.hardware-readonly.yaml
```

## 2026-05-17 Phase 9

### KPA500

Result: KPA500 basic serial protocol confirmed.

Command:

```powershell
.\target-msvc\debug\egb.exe serial-probe --port COM21 --baud 38400 --send "^RVM;" --timeout-ms 1000
```

Observed response:

```text
^RVM01.54;
```

Confirmed:

- Port: `COM21`
- Baud: `38400`
- Terminator: none
- Protocol: caret-prefixed KPA500 Programmer Reference command set
- Firmware parser: `^RVM01.54;` parses as firmware `01.54`

Pending KPA500 read-only commands:

- `^SN;`
- `^OS;`
- `^WS;`
- `^TM;`
- `^VI;`
- `^FL;`

Next KPA500 command:

```powershell
.\target-msvc\debug\egb.exe test-kpa --config .\config.hardware-readonly.yaml
```

### KAT500 Initial Baud Scan

```text
38400 printable=;
19200 printable=;
9600 printable=;
4800 open failed Access denied
```

This was inconclusive by itself because semicolon responses could be wake/null echo.

### KAT500 Read-Only Validation

Result: KAT500 read-only polling confirmed.

Confirmed:

- Port: `COM8`
- Terminator: none
- Baud: `38400`, `19200`, and `9600` all returned valid command responses
- Preferred configured baud: `38400`

Observed responses:

```text
; -> ;
RV; -> RV02.16;
SN; -> SN 3867;
AN; -> AN2;
BYP; -> BYPN;
MD; -> MDA;
TP; -> TP0;
FLT; -> FLT0;
VSWR; -> VSWR 1.00;
VFWD; -> VFWD 0;
```

Parsed state:

- firmware `02.16`
- serial `3867`
- antenna `2`
- bypass `false`
- mode `auto`
- tune power/status `0`
- fault `0`
- SWR `1.00`
- forward power `0`

Regression fixture:

```text
tests/fixtures/kat500-readonly-com8.txt
```

Next KAT500 commands:

```powershell
.\target-msvc\debug\egb.exe test-kat --config .\config.hardware-readonly.yaml
```

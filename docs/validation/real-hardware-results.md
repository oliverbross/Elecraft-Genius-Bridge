# Real Hardware Results

This file records actual KPA500/KAT500 serial validation results from Oliver's station.

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

### KAT500

Result: COM8 can be opened at multiple baud rates and semicolon wake/null probes return semicolon bytes, but no real KAT500 command response has been proven yet.

Observed baud-scan result:

```text
38400 printable=;
19200 printable=;
9600 printable=;
4800 open failed Access denied
```

Interpretation:

- Semicolon responses only prove wake/null bytes or echo-like behaviour.
- They do not prove that `RV;`, `SN;`, `AN;`, `BYP;`, or other command responses are available at that baud.
- The Phase 9 scanner now sends documented read-only queries after wake probes and classifies responses as `echo only`, `echo+data`, or `command response`.

Next KAT500 commands:

```powershell
.\target-msvc\debug\egb.exe baud-scan --port COM8
.\target-msvc\debug\egb.exe serial-probe-batch --port COM8 --baud 38400 --send ";,RV;,SN;,AN;,BYP;,MD;,TP;,FLT;,VSWR;,VFWD;" --timeout-ms 1000
```

If every query is `echo only` or `no response`, try:

```powershell
.\target-msvc\debug\egb.exe serial-probe-batch --port COM8 --baud 19200 --send ";,RV;,SN;,AN;,BYP;,MD;,TP;,FLT;,VSWR;,VFWD;" --timeout-ms 1000
.\target-msvc\debug\egb.exe serial-probe-batch --port COM8 --baud 9600 --send ";,RV;,SN;,AN;,BYP;,MD;,TP;,FLT;,VSWR;,VFWD;" --timeout-ms 1000
```

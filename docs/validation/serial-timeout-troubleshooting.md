# Serial Timeout Troubleshooting

Phase 8 corrected the first hardware timeout cause: opening `COM21` and `COM8` succeeded, but the active read-only commands were wrong.

## What Was Wrong

- KPA500 `ST;` and `RV;` were placeholder assumptions and timed out on real hardware.
- KPA500 uses caret-prefixed commands such as `^RVM;`, `^SN;`, `^OS;`, `^WS;`, `^TM;`, `^VI;`, and `^FL;`.
- KAT500 `ST;` is not a generic status command. It is SWR-threshold GET/SET.
- KAT500 can sleep. The utility wakes it by sending null commands, a single `;`, at about 100 ms intervals until semicolon responses are returned.

## First Probes

KPA500 firmware:

```powershell
.\target-msvc\debug\egb.exe serial-probe --port COM21 --baud 38400 --send "^RVM;" --timeout-ms 1000
```

KAT500 wake/null response:

```powershell
.\target-msvc\debug\egb.exe serial-probe --port COM8 --baud 38400 --send ";" --timeout-ms 1000
```

KAT500 baud scan:

```powershell
.\target-msvc\debug\egb.exe baud-scan --port COM8
```

Known station result: COM8 returned valid KAT500 command responses at `38400`, `19200`, and `9600`. Keep `38400` as the configured baud unless it fails.

KAT500 batch probe:

```powershell
.\target-msvc\debug\egb.exe serial-probe-batch --port COM8 --baud 38400 --send ";,RV;,SN;,AN;,BYP;,MD;,TP;,FLT;,VSWR;,VFWD;" --timeout-ms 1000
```

Optional KAT500 scan with only a version query after a wake response:

```powershell
.\target-msvc\debug\egb.exe baud-scan --port COM8 --version-query "RV;"
```

KPA500 baud scan, useful if the configured baud is uncertain:

```powershell
.\target-msvc\debug\egb.exe baud-scan --port COM21
```

## Interpreting Results

If opening the port fails:

- The COM port name is wrong.
- Another program has the port open.
- The USB serial driver is missing or unstable.

If the port opens but no bytes return at any baud:

- The device may be off or connected to the wrong port.
- The cable may be wrong.
- The device may require a different serial interface path.
- The device may be sleeping and need repeated `;` wake probes.

If `serial-probe` returns only `;`:

- The port and baud are probably correct.
- The device is responding to null commands.
- Move on to a documented read-only command, for example KPA500 `^RVM;` or KAT500 `RV;`.

If `baud-scan` reports `echo only`:

- The scanner received exactly the bytes it sent.
- Treat this as inconclusive until a query returns `command response` or `echo+data`.
- Run `serial-probe-batch` at the likely baud.

If `baud-scan` reports `command response`:

- The baud is likely correct.
- Save the transcript and run the corresponding high-level `test-kat` or `test-kpa`.

If KPA500 `^RVM;` works but `test-kpa` fails:

- Save the serial transcript from `logs/serial`.
- Check which read-only command timed out.
- Run `serial-probe` for that exact command.

If KAT500 wake works but `test-kat` fails:

- Run `serial-probe --send "RV;"`.
- Then probe `SN;`, `AN;`, `BYP;`, `MD;`, `TP;`, `FLT;`, `VSWR;`, and `VFWD;` individually.
- Save the transcript and firmware version.

## Transcript Files

`serial-probe` and `baud-scan` write transcript files under `logs/serial` by default. The driver tests also write one timestamped transcript per hardware session when `logging.serial_transcript_dir` is configured.

# Soak Testing

Phase 14 added a long-running operational soak mode:

```powershell
cargo run -p egb -- soak-test --config config.hardware-readonly.yaml --duration-hours 8
```

Phase 24 adds the shorter diagnostics-oriented evidence command expected by the GUI:

```powershell
.\target-msvc\debug\egb.exe evidence-test --config .\config.flex-injection-readonly.yaml --duration-minutes 10
```

On the current Windows development environment, use the checked script/toolchain setup if `cargo` is not on `PATH`.

## What It Starts

`soak-test`, `stability-test`, and `evidence-test` start the same bridge runtime as `egb run`:

- KPA500/KAT500 drivers according to the selected config
- PGXL/TGXL emulators
- stale-state watchdog
- optional localhost `/status` endpoint
- protocol and serial transcripts when configured

It does not enable any additional control commands. Hardware behavior is still governed by `dry_run`, `--allow-control`, and `--allow-rf-risk` on the separate test commands.

## Periodic Summary

`soak-test` prints every 60 seconds. `evidence-test` prints every 30 seconds and writes a complete evidence folder and ZIP in `diagnostics\runs`.

- elapsed runtime
- amp/tuner connection state
- successful and failed serial poll counts
- reconnect counts
- stale-state transition counts
- average and maximum poll latency
- active PGXL/TGXL client counts
- PGXL/TGXL sessions seen during the run
- SmartSDR/Flex tuner appeared/disappeared counters
- Flex ping success/failure counters
- protocol unknown-command counters

These summaries are intended to prove that AetherSDR polling and Elecraft serial polling can run for hours without hidden disconnect loops.

If no PGXL/TGXL direct client connects during `evidence-test`, the report includes a warning. This is expected when only the Flex-side SmartSDR tuner path is being exercised, but it means the direct TGXL reconnect loop was not captured.

## Transcript Rotation

Set:

```yaml
logging:
  serial_transcript_dir: logs/serial
  protocol_transcript_dir: logs/protocol
  transcript_rotate_bytes: 1048576
```

Serial and protocol transcripts rotate per device/client session when the configured byte limit is reached. Rotation failures are logged and transcript writing is disabled for that session; serial polling and client sessions continue.

## Recommended Runs

Start with mock mode:

```powershell
cargo run -p egb -- soak-test --config config.mock.yaml --duration-hours 1
```

Then run hardware read-only:

```powershell
cargo run -p egb -- soak-test --config config.hardware-readonly.yaml --duration-hours 4
```

For SmartSDR reconnect capture:

```powershell
.\target-msvc\debug\egb.exe evidence-test --config .\config.flex-injection-readonly.yaml --duration-minutes 10
```

For overnight testing, keep `dry_run: true`, leave RF-risk commands disabled, and save serial transcripts plus `/status` snapshots.

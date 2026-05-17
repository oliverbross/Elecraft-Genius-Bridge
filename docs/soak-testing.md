# Soak Testing

Phase 14 adds a long-running operational test mode:

```powershell
cargo run -p egb -- soak-test --config config.hardware-readonly.yaml --duration-hours 8
```

On the current Windows development environment, use the checked script/toolchain setup if `cargo` is not on `PATH`.

## What It Starts

`soak-test` starts the same bridge runtime as `egb run`:

- KPA500/KAT500 drivers according to the selected config
- PGXL/TGXL emulators
- stale-state watchdog
- optional localhost `/status` endpoint
- protocol and serial transcripts when configured

It does not enable any additional control commands. Hardware behavior is still governed by `dry_run`, `--allow-control`, and `--allow-rf-risk` on the separate test commands.

## Periodic Summary

Every 60 seconds the command logs and prints:

- elapsed runtime
- amp/tuner connection state
- successful and failed serial poll counts
- reconnect counts
- stale-state transition counts
- average and maximum poll latency
- active PGXL/TGXL client counts
- protocol unknown-command counters

These summaries are intended to prove that AetherSDR polling and Elecraft serial polling can run for hours without hidden disconnect loops.

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

For overnight testing, keep `dry_run: true`, leave RF-risk commands disabled, and save serial transcripts plus `/status` snapshots.

# Protocol Correction Workflow

This project changes PGXL/TGXL protocol behaviour only after captured evidence is documented.

## 1. Run Mock Bridge With Trace Enabled

Use `config.mock.yaml` and change `server.bind_ip` to the Windows LAN IP if AetherSDR runs on another machine.

```powershell
scripts\windows\run-mock.ps1
```

Required logging:

```yaml
logging:
  level: debug
  protocol_trace: true
  protocol_transcript_dir: logs/protocol
```

## 2. Connect AetherSDR

Configure manual IP:

- PGXL: Windows bridge IP, port `9008`
- TGXL: Windows bridge IP, port `9010`

Exercise only the UI controls needed for the current validation question.

## 3. Inspect Transcripts

For each session, inspect the timestamped files in `logs/protocol/`.

Record:

- server-first `V` greeting
- all inbound `C...` commands
- all outbound `R...` responses
- unsolicited `S...` state pushes
- disconnect point, if any

## 4. Compare Against Parser Tests

Check the relevant parser/formatter tests before changing implementation:

- `crates/pgxl-emulator/src/lib.rs`
- `crates/tgxl-emulator/src/lib.rs`

Any line accepted by AetherSDR should have a golden test. Any guessed field must stay marked `TODO` or `UNVERIFIED`.

## 5. Update Docs First

Update the source-of-truth protocol notes:

- `docs/protocol-analysis/pgxl.md`
- `docs/protocol-analysis/tgxl.md`
- `docs/validation/aethersdr-session-report-template.md`, copied to a session-specific report if useful

Do not change emulator behaviour from memory.

## 6. Add Or Adjust Golden Tests

Add stable sample messages from the transcript. Keep the test focused on the smallest observed behaviour.

If AetherSDR accepts multiple formats, document which one was captured and which one the emulator emits.

## 7. Change Emulator Implementation

Only after docs and tests are updated:

1. Adjust parser/formatter code.
2. Run the full check set.
3. Repeat AetherSDR validation.
4. Attach transcript paths or excerpts to the session report.

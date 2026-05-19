# Evidence Bundles

Phase 24 adds automatic evidence folders for bridge runs, soak runs, evidence tests, and hardware test commands.

## Recommended Command

```powershell
.\target-msvc\debug\egb.exe evidence-test --config .\config.flex-injection-readonly.yaml --duration-minutes 10
```

`stability-test` is kept with the same evidence behaviour.

## Output

Each run creates:

```text
diagnostics\runs\YYYYMMDD-HHMMSS-<mode>\
diagnostics\runs\YYYYMMDD-HHMMSS-<mode>.zip
```

## Files

- `egb-run.log`
- `status-start.json`
- `status-end.json`
- `status-samples.jsonl`
- `flex-rx.log`
- `flex-tx.log`
- `pgxl-protocol.log`
- `tgxl-protocol.log`
- `kpa500-serial.log`
- `kat500-serial.log`
- `client-sessions.jsonl`
- `disconnect-events.jsonl`
- `warnings-errors.log`
- `config-effective.yaml`
- `command.txt`
- `summary.md`
- `pgxl-vs-tgxl-analysis.md`
- `stability-report.json` for bounded evidence/stability tests

Start with `summary.md`. For SmartSDR ATU instability, inspect `disconnect-events.jsonl`, then the surrounding `flex-rx.log` and `flex-tx.log` lines.

For PGXL direct connection failures, inspect `pgxl-vs-tgxl-analysis.md` first, then `pgxl-protocol.log`. PGXL protocol lines include raw hex framing to catch CR/LF, spacing, and empty-field differences.

# PGXL Direct Connection Debugging

Phase 25 adds PGXL-specific handshake and framing evidence for AetherSDR "Not connected" cases.

Run:

```powershell
.\target-msvc\debug\egb.exe evidence-test --config .\config.smartsdr-stability.yaml --duration-minutes 10
```

Files to inspect:

- `summary.md` for likely-cause heuristics.
- `pgxl-vs-tgxl-analysis.md` for side-by-side PGXL/TGXL session counts and disconnect reasons.
- `pgxl-protocol.log` for PGXL RX/TX lines with `raw_hex=...`.
- `logs/protocol/pgxl-session-*.log` for per-session PGXL state-machine transcripts.

Compatibility profiles:

- `strict`: minimal protocol fields.
- `aethersdr`: conservative AetherSDR direct TCP fields.
- `smartsdr`: currently same direct TCP shape as AetherSDR, reserved for SmartSDR-specific deltas.
- `permissive`: accepts the broader EGB status shape.

Current likely failure modes:

- AMP pane visible but no PGXL TCP session: radio-side amplifier injection works, but AetherSDR is not opening or not targeting the PGXL direct socket.
- PGXL TCP session starts then disconnects: inspect raw framing, first command ordering, unknown commands, and status field formatting.
- TGXL remains visible while PGXL says not connected: compare `pgxl-protocol.log` with `tgxl-protocol.log` in the same evidence bundle.

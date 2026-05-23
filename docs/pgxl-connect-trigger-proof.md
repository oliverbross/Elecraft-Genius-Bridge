# PGXL Connect Trigger Proof

Phase 60 stops treating the 30-40 second PGXL delay as a socket bug until evidence proves a socket attempt exists.

## What EGB Can Prove

EGB logs the PGXL listener startup and every accepted PGXL client session. If `pgxl_session_started_count` stays `0` until the delayed connection timestamp, the Windows listener did not accept an earlier PGXL TCP connection. In that case the delay is upstream: AetherSDR did not open TCP 9008 yet, or the OS never completed the connection.

Rust application code cannot reliably observe raw TCP SYN packets before `accept()` without packet capture. If we need SYN-level proof, capture with Wireshark or Windows `pktmon` while running the same evidence test.

## Evidence Files

Use the evidence bundle from `egb pgxl-trigger-strategy-test` or `egb operational-gap-test`.

- `listener-startup.log`: proves PGXL/TGXL listeners were ready.
- `client-sessions.jsonl`: records the first accepted PGXL session timestamp.
- `pgxl-protocol.log`: records the first command received after accept.
- `amplifier-reannounce.log`: records the startup burst count and strategy.
- `amplifier-status-lines.log`: records amplifier create/status lines used as AetherSDR triggers.
- `pgxl-delayed-connect-analysis.md`: summarizes TGXL first connect, PGXL first connect, and the delta.

## Trigger Strategy Test

Run one strategy at a time:

```powershell
.\target\release\egb.exe pgxl-trigger-strategy-test --config .\config.aethersdr-last-known-good-real-controls.yaml --strategy rapid_sub_only --duration-seconds 120
```

Valid strategies:

- `current`: last-known-good startup burst behaviour.
- `rapid_sub_only`: fast bounded `sub amplifier all` refreshes only.
- `reannounce_status_only`: bounded status reannounce evidence without subscription spam.
- `reannounce_create_style_status`: bounded create-style reannounce evidence for comparison.
- `no_burst`: no startup burst.

If PGXL still connects only after the same delay and `client-sessions.jsonl` has no earlier PGXL accept, the remaining trigger is AetherSDR-side eligibility/retry timing, not EGB accepting/rejecting TCP.

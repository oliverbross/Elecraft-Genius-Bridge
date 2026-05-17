# Protocol Replay Harness

This directory is reserved for the Phase 1/2 replay harness design.

No executable harness has been implemented yet. The initial task explicitly required protocol research and planning before emulator implementation.

See:

- `docs/architecture/replay-harness-design.md`
- `docs/protocol-analysis/pgxl.md`
- `docs/protocol-analysis/tgxl.md`

Planned responsibilities:

- Replay PGXL/TGXL handshakes.
- Serve scripted responses to AetherSDR.
- Log transcripts with timestamps.
- Convert real packet captures into repeatable scenarios.
- Validate emulator behavior before Elecraft serial integration.


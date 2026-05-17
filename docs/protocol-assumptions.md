# Protocol Assumptions Registry

Every inferred behaviour stays here until validated by source, transcript, or hardware.

| Assumption | Source | Confidence | Validation status | Transcript evidence | TODO |
| --- | --- | --- | --- | --- | --- |
| PGXL direct TCP listens on port `9008`. | Phase 1 AetherSDR source inspection | High | Mock smoke tested | `docs/validation/local-tcp-smoke-test.md` | Validate with current AetherSDR build |
| TGXL direct TCP listens on port `9010`. | Phase 1 AetherSDR source inspection | High | Mock smoke tested | `docs/validation/local-tcp-smoke-test.md` | Validate with current AetherSDR build |
| PGXL/TGXL send server-first `V<version>` greeting. | Phase 1 AetherSDR source inspection and local smoke test | High | Mock smoke tested | `docs/validation/local-tcp-smoke-test.md` | Confirm no version format strictness in real AetherSDR |
| PGXL `info` response fields used by AetherSDR are `model`, `serial_num`, and `version`. | Inferred from Phase 1 source inspection | Medium | Replay tested only | `tests/replay/pgxl-polling-session.txt` | Capture real AetherSDR session |
| TGXL `info` response fields used by AetherSDR are `model`, `serial_num`, `version`, and `one_by_three`. | Inferred from Phase 1 source inspection | Medium | Replay tested only | `tests/replay/tgxl-polling-session.txt` | Capture real AetherSDR session |
| PGXL status can include `connection_state` without breaking AetherSDR parsing. | Inferred from key/value protocol style | Low | Not AetherSDR validated | None yet | Validate with AetherSDR; remove or adapt if UI rejects it |
| TGXL status can include `connection_state` and `fault` without breaking AetherSDR parsing. | Inferred from key/value protocol style | Low | Not AetherSDR validated | None yet | Validate with AetherSDR; remove or adapt if UI rejects it |
| AetherSDR PGXL direct TCP does not require operate/standby commands for MVP. | Phase 1 source observation | Medium | Not AetherSDR validated | None yet | Confirm with UI session |
| TGXL `autotune`, `activate ant=N`, and `tune relay=<0|1|2> move=<+1|-1>` are sufficient observed control commands. | Phase 1 source observation | Medium | Replay tested only | `tests/replay/tgxl-polling-session.txt` | Validate with UI controls |
| KPA500 version query `RV;` is safe read-only. | Placeholder mapping | Low | Unverified | None yet | Replace with official command reference or transcript |
| KAT500 version query `RV;` is safe read-only. | Placeholder mapping | Low | Unverified | None yet | Replace with official command reference or transcript |
| KPA500 status query `ST;` is safe read-only. | Placeholder mapping | Low | Unverified | None yet | Replace with official command reference or transcript |
| KAT500 status query `ST;` is safe read-only. | Placeholder mapping | Low | Unverified | None yet | Replace with official command reference or transcript |

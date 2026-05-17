# Protocol Assumptions Registry

Every inferred behaviour stays here until validated by source, transcript, or hardware.

| Assumption | Source | Confidence | Validation status | Transcript evidence | TODO |
| --- | --- | --- | --- | --- | --- |
| PGXL direct TCP listens on port `9008`. | Phase 1 AetherSDR source inspection | High | Mock smoke tested | `docs/validation/local-tcp-smoke-test.md` | Validate with current AetherSDR build |
| TGXL direct TCP listens on port `9010`. | Phase 1 AetherSDR source inspection | High | Mock smoke tested | `docs/validation/local-tcp-smoke-test.md` | Validate with current AetherSDR build |
| PGXL/TGXL send server-first `V<version>` greeting. | Phase 1 AetherSDR source inspection and local smoke test | High | Mock smoke tested | `docs/validation/local-tcp-smoke-test.md` | Confirm no version format strictness in real AetherSDR |
| PGXL `info` response fields accepted by AetherSDR include `model`, `serial_num`, and `version`. | AetherSDR transcript and replay test | High | AetherSDR polling validated | `logs/protocol/pgxl-1779018198497-127_0_0_1_55157.log` | Validate richer `firmware`/`capabilities` fields |
| TGXL `info` response fields accepted by AetherSDR include `model`, `serial_num`, `version`, and `one_by_three`. | AetherSDR transcript and replay test | High | AetherSDR polling validated | `logs/protocol/tgxl-1779018197185-127_0_0_1_55149.log` | Validate richer `firmware`/`capabilities` fields |
| Direct PGXL `firmware` and `capabilities` info fields are tolerated by AetherSDR. | Compatibility enrichment, not source-proven | Low | Replay tested only | None yet | Validate with AetherSDR after Phase 5 |
| Direct TGXL `firmware` and `capabilities` info fields are tolerated by AetherSDR. | Compatibility enrichment, not source-proven | Low | Replay tested only | None yet | Validate with AetherSDR after Phase 5 |
| PGXL status can include `connection_state` without breaking AetherSDR parsing. | Inferred from key/value protocol style | Low | Not AetherSDR validated | None yet | Validate with AetherSDR; remove or adapt if UI rejects it |
| TGXL status can include `connection_state` and `fault` without breaking AetherSDR parsing. | Inferred from key/value protocol style | Low | Not AetherSDR validated | None yet | Validate with AetherSDR; remove or adapt if UI rejects it |
| AetherSDR PGXL direct TCP does not require operate/standby commands for MVP. | Phase 1 source observation | Medium | Not AetherSDR validated | None yet | Confirm with UI session |
| TGXL `autotune`, `activate ant=N`, and `tune relay=<0|1|2> move=<+1|-1>` are sufficient observed control commands. | Phase 1 source observation | Medium | Replay tested only | `tests/replay/tgxl-polling-session.txt` | Validate with UI controls |
| AetherSDR AMP/TUN applet tray visibility is gated by radio-side `amplifier` presence objects, not direct PGXL/TGXL TCP connection state alone. | Phase 5 source inspection of `AppletPanel`, `MainWindow`, and `RadioModel` | High | Matches observed hidden applets despite stable direct polling | `docs/analysis/aethersdr-session-sequence.md` | Decide between AetherSDR patch and Flex radio API presence proxy |
| KPA500 version query `RV;` is safe read-only. | Placeholder mapping | Low | Unverified | None yet | Replace with official command reference or transcript |
| KAT500 version query `RV;` is safe read-only. | Placeholder mapping | Low | Unverified | None yet | Replace with official command reference or transcript |
| KPA500 status query `ST;` is safe read-only. | Placeholder mapping | Low | Unverified | None yet | Replace with official command reference or transcript |
| KAT500 status query `ST;` is safe read-only. | Placeholder mapping | Low | Unverified | None yet | Replace with official command reference or transcript |

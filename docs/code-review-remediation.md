# Code Review Remediation

Phase 22 addressed the highest-risk correctness findings before further UI or protocol work.

## Fixed

| Item | Status | Notes |
| --- | --- | --- |
| Bridge uptime | Fixed | Bridge start time is set when the runtime starts, not on the first `/status` request. |
| HTTP `Content-Length` | Fixed | The status server now uses `body.as_bytes().len()`. |
| Stale watchdog lock behaviour | Fixed | The watchdog reads first and only takes a write lock for real transitions. |
| LAN/private IP validation | Fixed | LAN, loopback, link-local, IPv6 ULA/link-local, and CGNAT checks are centralized in `egb-config`. |
| Duplicate `push_capability` | Fixed | The helper now lives in `bridge-core`. |
| Dead `send_dynamic_command` abstraction | Fixed | KAT500 dynamic commands use the normal command path directly. |
| `AmpState.connected` divergence in `/status` | Fixed | `/status` reports connectivity from `connection_state`. The legacy boolean remains for compatibility. |
| Flex backoff reset | Fixed | Backoff resets after a live Flex session. |
| `parse_amplifier_status` false positives | Fixed | Amplifier status parsing ignores `R...` command responses. |
| Bounded Flex pending commands | Fixed | Pending Flex commands now expire by TTL and max size; counts are exposed in `/status`. |
| Duplicate KAT500 discovery baud | Fixed | Discovery baud list is unique: `38400, 19200, 9600, 4800`. |
| `watts_to_dbm(0.0)` | Fixed | Zero or negative power now reports `-120.0 dBm`, not `0.0 dBm`. |

## Deferred

| Item | Reason |
| --- | --- |
| Buffered serial reader rewrite | Requires persistent per-port buffering so extra bytes from multi-response reads are not dropped. Current prefix matching remains intact, but the byte-at-a-time reader can still add jitter. |
| Split `egb/main.rs` | Deferred to avoid mixing a large mechanical refactor with SmartSDR stability fixes. |
| Dependency upgrades | Deferred; see `docs/dependency-review.md`. |

## Tests Added

- LAN scope helper coverage for IPv4 loopback/private/link-local/CGNAT and IPv6 loopback/ULA/link-local.
- Flex amplifier parser regression proving `R...|amplifier ...` response lines are ignored.
- Existing golden protocol tests were updated for the corrected zero-power dBm sentinel.

## Risk Assessment

The remaining largest engineering risk is serial read buffering. Handle it as an isolated refactor with mock serial fixtures covering partial reads and multiple responses in one OS buffer.

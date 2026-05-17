# Operational Readiness Matrix

| Area | Status | Evidence | Remaining Work |
| --- | --- | --- | --- |
| AetherSDR direct sockets | Ready for continued validation | PGXL/TGXL sockets connect and poll stably | Applet visibility is separate and unresolved |
| Mock mode | Ready | Parser, golden, and replay tests pass | Longer soak with real AetherSDR still useful |
| KPA500 read-only | Ready | COM21 38400 and caret read-only commands verified except serial-number transcript status | Continue collecting longer transcripts |
| KPA500 standby | Ready for LAN/local safe-control testing | `^OS0;` is ack-less and verifies via `^OS; -> ^OS0;` | Include in soak-adjacent operator test, not unattended |
| KPA500 operate | Not ready | Command classified RF-risk | Requires controlled operate workflow, RF inhibit, dummy load, and explicit `--allow-rf-risk` |
| KAT500 read-only | Ready | COM8 38400 read-only commands verified | Longer polling soak |
| KAT500 control | Not ready | Control commands intentionally blocked | Validate official command semantics and RF safety |
| Serial reconnect | Implemented, needs soak evidence | Exponential backoff and degraded state transitions added | Physically test USB disconnect/reconnect |
| Stale-state handling | Implemented, needs soak evidence | Watchdog marks degraded and tracks transition counts | Validate with paused/disconnected devices |
| Metrics endpoint | Local diagnostic ready | `/status` returns runtime counters and stale durations | Keep loopback-only; no auth yet |
| Transcript rotation | Implemented, needs soak evidence | Serial and protocol transcripts rotate by configured byte limit | Validate disk growth during long sessions |
| Windows service | Not ready | Scripts and docs only | Install/uninstall service and recovery policy |
| WAN exposure | Not ready | No auth/TLS/rate limiting | Use LAN/VPN only until security phase |
| Applet visibility | Not ready | Direct sockets stable but panels hidden | Resolve Flex radio-side presence or proxy strategy |

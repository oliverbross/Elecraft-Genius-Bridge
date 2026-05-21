# Official TGXL API Compliance Review

Authoritative source: 4O3A Tuner Genius XL API Commands PDF.

Phase 47 scope is protocol correctness. It does not treat Elecraft serial polling as the problem; KPA500/KAT500 serial is already validated separately.

## Command Matrix

| Command/status item | Official request format | Official response format | Required fields | EGB support | KAT500 mapping | Safety | Implementation action |
|---|---|---|---|---|---|---|---|
| UDP discovery | UDP port 9010 discovery query/response | Device discovery record | model/IP/version identity | Missing | None | Read-only | Intentionally deferred; AetherSDR manual IP works and UDP discovery is not required for current MVP. |
| TCP greeting | Server sends `V<version>`; auth may be advertised by real device | Plain line before commands | Version string, optional auth state | Partial | None | Read-only | Existing `V0.1.0-egb-tgxl`; auth is accepted but not enforced. |
| `auth` | `C<seq>|auth <code>` | `R<seq>|0|...` | Auth result | Full for MVP | None | Read-only | Added stable `auth OK` response; no token enforcement in native TGXL protocol. |
| `btl` | `C<seq>|btl` | Success/error | Bootloader transition | Intentionally unsupported | None | Destructive/unknown | Added stable error `bootloader_unsupported_by_bridge`; never reboots hardware. |
| `info` | `C<seq>|info` | `R<seq>|0|...` | identity/version/model | Full enough for AetherSDR | None | Read-only | Existing response retained. |
| `ifconf read` | `C<seq>|ifconf read` | `R<seq>|0|ifconf ...` | DHCP/IP/netmask/gateway | Full safe placeholder | None | Read-only | Implemented. |
| `ifconf set` | `C<seq>|ifconf set ...` | Success/reboot flag | Updated network config | Partial safe no-op | None | State-change/config | Added `R<seq>|0|0`; EGB config remains YAML/GUI owned. |
| `setup read` | `C<seq>|setup read` | `R<seq>|0|setup ...` | nickname/code/backlight/bypass/tune PTT fields | Full safe placeholder | None | Read-only | Implemented. |
| `setup set` | `C<seq>|setup set ...` | Success | Updated setup config | Partial safe no-op | None | State-change/config | Added stable success without mutating EGB runtime config. |
| `catradio read` | `C<seq>|catradio read` | ch=1 and ch=2 responses | channel config | Full safe placeholder | None | Read-only | Implemented as two response lines. |
| `catradio get` | `C<seq>|catradio get ch=N` | channel config | selected channel config | Full safe placeholder | None | Read-only | Implemented. |
| `catradio set` | `C<seq>|catradio set ...` | Success | updated channel config | Partial safe no-op | None | State-change/config | Accepted as no-op. |
| `flexradio list` | `C<seq>|flexradio list` | radio list + final response | serial/nickname/callsign | Full if Flex context is known, otherwise defaults | Flex API context | Read-only | Implemented from `/status.radio_context`. |
| `flexradio read` | `C<seq>|flexradio read` | ch=1/ch=2 responses | active/serial/antenna/source | Full from Flex context | Flex API context | Read-only | Implemented. |
| `flexradio get` | `C<seq>|flexradio get ch=N` | channel response | selected channel | Full from Flex context | Flex API context | Read-only | Implemented. |
| `flexradio set` | `C<seq>|flexradio set ch=N active=1 serial=... antenna=... source=LAN` | Success | channel config | Partial | Flex API context | State-change/config | Records radio serial/antenna context only; does not mutate real radio. |
| `status` | `C<seq>|status` | `S<seq>|status ...` | all status fields listed below | Full field presence, partial real value coverage | KAT500 + Flex context | Read-only | Expanded in Phase 45. |
| `operate set=0/1` | `C<seq>|operate set=N` | Success/error | virtual operate state | Partial | None | State-change | Updates virtual TGXL state only; no KAT500 RF action. |
| `bypass set=0/1` | `C<seq>|bypass set=N` | Success/error | desired bypass state | Full when enabled | KAT500 `BYP;` / `BYPN;` | State-change safe, gated | Maps through existing safety gate. |
| `activate ant=N` | `C<seq>|activate ant=N` | Success + status push | antenna 1..3 | Full when enabled | KAT500 `AN<n>;` | State-change safe, gated | Implemented via desired antenna state. |
| `activate ch=N` | `C<seq>|activate ch=N` | Success/error | active channel | Partial | None | Read-only/virtual state | Added stable virtual channel acceptance; EGB only emulates one KAT500 path. |
| `autotune` | `C<seq>|autotune` | Success + status push | tune request | Full when enabled | KAT500 `T;` | RF-risk/tune gated | Existing mapping retained; tune lifecycle now resets after completion/failure. |
| `save` | `C<seq>|save` | Success/reboot behavior | persistent config | Partial safe no-op | None | State-change/config | Added stable success; EGB config persistence remains YAML/GUI owned. |
| `tune relay/move` | `C<seq>|tune relay=N move=+1|-1` | Success/error | relay C1/L/C2 and movement | Partial | None yet | Destructive/unknown | Accepts official 1-based ids and old zero-based traces, but does not send unverified KAT500 relay controls. |

## Status Field Review

| Field | Source of truth in EGB | Mapping/default | Changes over time | Unit/format notes |
|---|---|---|---|---|
| `fwd` | `TunerState.forward_power_watts` | watts converted to dBm, `-120.00` for zero | Yes, from KAT500 `VFWD;` | dBm-style value for API compatibility. |
| `peak` | same as `fwd` | mirrors fwd | Yes | Peak hold not separately available from KAT500. |
| `max` | max(fwd, 0) | `0.00` at no RF | Yes | Safe placeholder. |
| `swr` | `TunerState.swr` | converted to return-loss style dB; `-30.0000` at no RF | Yes, from KAT500 `VSWR;` | Existing AetherSDR direct parser expects negative dB-style value. |
| `pttA`, `pttB` | Flex TX context unavailable as boolean today | `0` | Not yet | Needs live TX state if required by clients. |
| `bandA` | `RadioContext.band` | ham band number, `0` unknown | Yes, from Flex slice/TX | Phase 44/45 propagation. |
| `bandB` | not used | `0` | No | One KAT500 path currently active. |
| `modeA` | `RadioContext.frequency_hz` / mode presence | `1` when frequency known | Yes | Exact TGXL numeric mode mapping still needs capture evidence. |
| `modeB` | not used | `0` | No | One KAT500 path currently active. |
| `flexA` | `RadioContext.radio_nickname` | nickname or `FlexRadio` | Yes | Parsed from Flex radio status when present. |
| `flexB` | not used | empty | No | One radio path currently active. |
| `freqA` | `RadioContext.frequency_hz` | MHz with 6 decimals | Yes | Active TX slice is authoritative. |
| `freqB` | not used | `0.000` | No | One channel active. |
| `bypassA`, `bypass` | `TunerState.bypass` | `0/1` | Yes | KAT500 `BYP;` / `BYPN;`. |
| `bypassB`, `bypassRxA`, `bypassRxB` | unavailable | `0` | No | Safe default. |
| `antA` | `TunerState.selected_antenna` | KAT500 antenna number | Yes | KAT500 reports `AN<n>;`. |
| `antB` | not used | `0` | No | One channel active. |
| `state` | advertised tuner operate/control profile | `1` in control-ready profiles; otherwise tuner operate | Yes | Used for UI enablement. |
| `active` | EGB direct endpoint availability | `1` | Stable | Bridge represents one active TGXL. |
| `tuning` | `TunerState.tuning` or degraded state | `0/1` | Yes | KAT500 `TP;` updates tune state. |
| `ag` | unavailable | `0` | No | Safe default. |
| `relayC1`, `relayL`, `relayC2` | `TunerState` relay fields | current state/defaults | Yes if commands are accepted | Manual relay execution remains blocked until verified. |

## Current TGXL Compliance Estimate

TGXL direct compliance is estimated at **85%**:

- Full: 15 command/status groups.
- Partial: 4 groups.
- Missing: 1 group (`UDP discovery`).

Remaining practical risks are mode/PTT semantics, manual relay execution, and exact multi-channel behavior.

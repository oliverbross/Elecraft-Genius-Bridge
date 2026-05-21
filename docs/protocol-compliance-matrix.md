# Protocol Compliance Matrix

Phase 45 source baseline:

- FlexRadio Power Genius XL API Documentation: `https://www.flexradio.com/documentation/power-genius-xl-api-documentation/`
- 4O3A Tuner Genius XL API Commands: `https://4o3a.com/images/downloads/api/TG_XL_API_Commands.pdf`
- 4O3A PGXL Amplifier-to-Radio API Documentation: `https://4o3a.com/images/downloads/api/PGXL-Amplifier-to-Radio-API-Documentation.pdf`

## TGXL Direct TCP

| Official behavior | EGB before Phase 45 | Phase 45 implementation | Remaining work |
| --- | --- | --- | --- |
| Greeting `V<version>` after TCP connect | Implemented | Kept | Authentication mode is not implemented. MVP remains LAN/local only. |
| Commands use `C<seq>|<command> <data>` and replies use `R<seq>|<code>|<message>` | Implemented for basic commands | Kept | None for framing. |
| `status` reply starts with `S<seq>|status ...` | Returned simplified `R<seq>|0|operate=...` | Changed to `S<seq>|status ...` | Live AetherSDR validation required. |
| Status fields include `fwd`, `peak`, `max`, `swr`, `pttA`, `bandA`, `modeA`, `flexA`, `freqA`, `bypassA`, `bypassRxA`, `antA`, `pttB`, `bandB`, `modeB`, `flexB`, `freqB`, `bypassB`, `bypassRxB`, `antB`, `state`, `active`, `tuning`, `bypass`, `ag`, `relayC1`, `relayL`, `relayC2` | Only `operate`, `bypass`, `tuning`, relay fields, `antA`, `fwd`, `swr` | Full field list emitted | Exact `bandA` numeric encoding remains inferred; EGB uses amateur band number. |
| `info` returns `info serial=<serial> version=<version> nickname=<name> 3way=<0/1>` | Returned model-centric compatibility fields | Official fields added, compatibility fields retained for AetherSDR (`model`, `serial_num`, `one_by_three`) | Remove compatibility fields only if AetherSDR source no longer needs them. |
| `ifconf read` | Missing | Implemented with safe placeholder network defaults | Real TGXL network config is not meaningful for EGB. |
| `setup read` | Missing | Implemented with stable documented defaults | Set/save paths intentionally not implemented. |
| `catradio read/get` | Missing | Implemented read-only documented defaults | Set path accepts but does not alter hardware. |
| `flexradio list/read/get/set` | Missing | Implemented using Flex radio context where available | Radio serial/nickname depends on Flex status availability. |
| `operate set=0/1` | Missing | Accepted as TGXL state request | No direct KAT500 command is mapped for operate. |
| `bypass set=0/1` | Missing | Maps to desired KAT500 `BYP;` / `BYPN;` through existing safety gates | Requires live hardware validation with controls enabled. |
| `activate ch=1/2` | Missing | Not yet implemented | Existing `activate ant=N` remains implemented. |
| `activate ant=1/2/3` | Implemented | Kept | Requires controls enabled to actuate KAT500 antenna command. |
| `autotune` | Implemented | Kept; now paired with Flex band/frequency evidence | KAT500 cannot be explicitly forced to a band with current verified command map. |

## Flex Radio Context

| Official behavior | EGB before Phase 45 | Phase 45 implementation | Remaining work |
| --- | --- | --- | --- |
| PGXL tracks band/frequency from Flex slice subscriptions | Partial | `sub slice all` and `sub tx all` maintained; active TX slice updates `/status.radio_context` | Need live band-change validation from 15m to 20m. |
| Flex radio identity feeds TGXL Flex integration config | Missing | Parses `radio` status lines for serial/nickname/callsign when present | Need confirm exact Flex TCP `radio` status shape from live captures. |

## PGXL Amplifier-to-Radio

| Official behavior | EGB status | Remaining work |
| --- | --- | --- |
| Connect to Flex API and receive client handle | Implemented | None known. |
| `amplifier create ip=<ip> port=9008 model=PowerGeniusXL serial_num=<serial> ant=...` | Implemented | Use LAN-reachable EGB IP for real AetherSDR/SmartSDR clients. |
| Create meters `FWD`, `RL`, `DRV`, `ID`, `TEMP` | Implemented | Meter value publishing remains limited by available Flex API evidence. |
| Create AMP interlock | Implemented | Validate antenna map and SmartSDR interlock behavior live. |
| `keepalive enable` then periodic `ping` | Implemented | Verify ping ACK cadence in live evidence. |
| Subscribe `sub amplifier all`, `sub slice all`, `sub tx all` | Implemented | None known. |
| Do not recreate amplifier unless Flex session reconnects | Intended | Phase 45 adds removal counters to prove whether Flex removes the object. |
| PGXL direct status is stable and non-contradictory | Implemented from KPA500 shared state | Official PGXL Windows/direct status field set is still less detailed than TGXL PDF; continue validating against AetherSDR transcript. |

## Known Compatibility Workarounds

`pgxl_connect_assist` sends a Flex-side `amplifier set <handle> operate=1` to trigger AetherSDR PGXL TCP connection. It does not send `^OS1;` to KPA500. Flex has rejected this in some runs with `500000A7`, so it is a compatibility workaround, not the primary lifecycle model.

The preferred stable path is the documented PGXL lifecycle: amplifier create, meters, interlock, keepalive, subscriptions, and a stable LAN-reachable `ip`/`port`.

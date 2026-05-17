# Protocol Field Matrix

Status labels:

- `validated`: observed in AetherSDR transcripts or source with stable behaviour.
- `inferred`: derived from AetherSDR source or UI needs, not yet validated against hardware.
- `unverified`: placeholder or compatibility probe.

## PGXL Direct TCP

| Response | Field | Source | Confidence | Observed By AetherSDR | Mandatory Guess | Current Status |
| --- | --- | --- | --- | --- | --- | --- |
| `V` | version text | `PgxlConnection.cpp` waits for `V` first | High | Yes | Mandatory | Implemented |
| `info` | `model` | Current transcript and AetherSDR parser | High | Yes | Likely mandatory | Implemented as `PowerGeniusXL` |
| `info` | `serial_num` | Existing Phase 1 docs; key/value parser tolerates it | Medium | Yes | Unknown | Implemented as `EGB-PGXL` |
| `info` | `version` | Existing Phase 1 docs; key/value parser tolerates it | Medium | Yes | Unknown | Implemented |
| `info` | `firmware` | Compatibility enrichment | Low | Not yet | Optional guess | Implemented, TODO/UNVERIFIED |
| `info` | `capabilities` | Compatibility enrichment | Low | Not yet | Optional guess | Implemented, TODO/UNVERIFIED |
| `status` | `state` | `MainWindow.cpp` PGXL telemetry handling | High | Yes | Mandatory for UI state | Implemented |
| `status` | `peakfwd` | `MainWindow.cpp` converts dBm to watts | High | Yes | Mandatory for power display | Implemented |
| `status` | `swr` | `MainWindow.cpp` treats value as return loss | High | Yes | Mandatory for SWR display | Implemented |
| `status` | `temp` | `MainWindow.cpp` PGXL telemetry handling | High | Yes | Expected | Implemented |
| `status` | `id` | `MainWindow.cpp` PGXL telemetry handling | High | Yes | Expected | Implemented |
| `status` | `vac` | `MainWindow.cpp` PGXL telemetry handling | High | Yes | Expected | Implemented |
| `status` | `meffa` | `MainWindow.cpp` PGXL telemetry handling | High | Yes | Expected | Implemented |
| `status` | `fault` | Bridge degraded-state reporting | Low | Yes, tolerated by parser | Optional | Implemented |
| `status` | `connection_state` | Bridge degraded-state reporting | Low | Yes, tolerated by parser | Optional | Implemented |

## TGXL Direct TCP

| Response | Field | Source | Confidence | Observed By AetherSDR | Mandatory Guess | Current Status |
| --- | --- | --- | --- | --- | --- | --- |
| `V` | version text | `TgxlConnection.cpp` waits for `V` first | High | Yes | Mandatory | Implemented |
| `info` | `model` | Current transcript and AetherSDR parser | High | Yes | Likely mandatory | Implemented as `TunerGeniusXL` |
| `info` | `serial_num` | Existing Phase 1 docs; key/value parser tolerates it | Medium | Yes | Unknown | Implemented as `EGB-TGXL` |
| `info` | `version` | Existing Phase 1 docs; key/value parser tolerates it | Medium | Yes | Unknown | Implemented |
| `info` | `firmware` | Compatibility enrichment | Low | Not yet | Optional guess | Implemented, TODO/UNVERIFIED |
| `info` | `one_by_three` | `TunerModel.cpp` enables antenna controls | High | Yes | Likely mandatory for ANT 1/2/3 | Implemented |
| `info` | `capabilities` | Compatibility enrichment | Low | Not yet | Optional guess | Implemented, TODO/UNVERIFIED |
| `status` / `state` | `operate` | `TunerModel.cpp` and `TunerApplet.cpp` | High | Yes | Expected for state UI | Implemented |
| `status` / `state` | `bypass` | `TunerModel.cpp` and `TunerApplet.cpp` | High | Yes | Expected for state UI | Implemented |
| `status` / `state` | `tuning` | `TunerModel.cpp` and `TunerApplet.cpp` | High | Yes | Expected for tune UI | Implemented |
| `status` / `state` | `relayC1` | `TunerModel.cpp` | High | Yes | Expected for tuner controls | Implemented |
| `status` / `state` | `relayL` | `TunerModel.cpp` | High | Yes | Expected for tuner controls | Implemented |
| `status` / `state` | `relayC2` | `TunerModel.cpp` | High | Yes | Expected for tuner controls | Implemented |
| `status` / `state` | `antA` | `TunerModel.cpp` | High | Yes | Expected for antenna display | Implemented |
| `status` / `state` | `fwd` | `TunerModel.cpp` converts dBm to watts | High | Yes | Expected for power display | Implemented |
| `status` / `state` | `swr` | `TunerModel.cpp` treats value as return loss | High | Yes | Expected for SWR display | Implemented |
| `status` / `state` | `connection_state` | Bridge degraded-state reporting | Low | Yes, tolerated by parser | Optional | Implemented |
| `status` / `state` | `fault` | Bridge degraded-state reporting | Low | Yes, tolerated by parser | Optional | Implemented |

## Applet Activation Fields

These are not PGXL/TGXL direct TCP response fields. They are Flex radio-model presence fields consumed by AetherSDR before it shows applet tray buttons.

| Radio API object | Field | Source | Confidence | Current Implementation |
| --- | --- | --- | --- | --- |
| `amplifier <handle>` | `model=PowerGeniusXL` or other non-empty non-TGXL model | `RadioModel.cpp` amplifier parser | High | Not implemented |
| `amplifier <handle>` | `model=TunerGeniusXL` | `RadioModel.cpp` TGXL routing | High | Not implemented |
| `amplifier <handle>` | `ip=<addr>` | `RadioModel.cpp`, `TunerModel.cpp`, `MainWindow.cpp` auto-connect | High | Not implemented |
| `amplifier <handle>` | `operate` | `RadioModel.cpp`, `TunerModel.cpp` | High | Not implemented |
| `amplifier <handle>` | `bypass` | `TunerModel.cpp` | High | Not implemented |
| `amplifier <handle>` | `serial_num` | `TunerModel.cpp` | Medium | Not implemented |

Current conclusion: hidden applets are probably caused by missing radio API presence fields, not missing direct TCP polling fields.

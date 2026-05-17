# AetherSDR Session Sequence Analysis

Status: validated against local AetherSDR transcripts, source-inspected against `research/AetherSDR` commit `6d17b3bbda96b836762e7d40758a1fc3e14725f9`.

## Transcript Inputs

Latest successful direct-IP sessions inspected:

- `logs/protocol/pgxl-1779018198497-127_0_0_1_55157.log`
- `logs/protocol/tgxl-1779018197185-127_0_0_1_55149.log`

Both sessions show accepted TCP transport, accepted `V` greeting, stable polling, and no disconnect loop.

## PGXL Sequence

Observed first-command order:

```text
PGXL TX > V0.1.0-egb-pgxl
PGXL RX < C1|info
PGXL TX > R1|0|model=PowerGeniusXL serial_num=EGB-PGXL version=0.1.0-egb-pgxl
PGXL RX < C2|status
PGXL TX > R2|0|state=STANDBY peakfwd=0.0000 swr=32.2557 temp=32.0 id=0.0 vac=230 meffa=OK fault= connection_state=connected
PGXL RX < C3|status
```

Timing:

- First `info` follows the server `V` greeting immediately.
- First `status` follows `info` immediately.
- Steady-state `status` interval is approximately `200 ms`.
- No PGXL direct operate/standby command was observed in this session.
- No richer direct PGXL capability query was observed.

## TGXL Sequence

Observed first-command order:

```text
TGXL TX > V0.1.0-egb-tgxl
TGXL RX < C1|info
TGXL TX > R1|0|model=TunerGeniusXL serial_num=EGB-TGXL version=0.1.0-egb-tgxl one_by_three=1
TGXL RX < C2|status
TGXL TX > R2|0|operate=0 bypass=0 tuning=0 relayC1=20 relayL=35 relayC2=20 antA=0 one_by_three=1 fwd=0.0000 swr=-32.2557 connection_state=connected fault=
TGXL RX < C3|status
```

Timing:

- First `info` follows the server `V` greeting immediately.
- First `status` follows `info` immediately.
- Steady-state `status` interval is approximately `1000 ms`.
- No TGXL direct `autotune`, `activate ant=N`, or `tune relay=...` command was observed during the inspected passive session.
- No richer direct TGXL capability query was observed.

## Applet Visibility Finding

The direct PGXL/TGXL TCP connection classes emit connected state after the `V` greeting, but AetherSDR applet tray-button visibility is not driven by those direct sockets alone.

Source path:

- `src/gui/AppletPanel.cpp`: AMP and TUN applet buttons are created during startup but hidden.
- `src/gui/MainWindow.cpp`: `TunerModel::presenceChanged` drives `AppletPanel::setTunerVisible`.
- `src/gui/MainWindow.cpp`: `RadioModel::amplifierChanged` drives `AppletPanel::setAmpVisible` and direct PGXL auto-connect.
- `src/models/RadioModel.cpp`: radio-side `amplifier` status with non-empty `model` sets amplifier presence; `model=TunerGeniusXL` is routed to `TunerModel`.

Current conclusion: direct IP PGXL/TGXL transport is sufficient for connection and polling, but current AetherSDR applet visibility also requires Flex radio model presence records, normally delivered by radio API status lines such as:

```text
amplifier <handle> model=PowerGeniusXL ip=<addr> ...
amplifier <handle> model=TunerGeniusXL ip=<addr> ...
```

This explains why stable direct polling can coexist with hidden applets. The bridge is not yet injecting or proxying Flex radio-side presence, and doing so is a separate design decision from PGXL/TGXL direct socket emulation.

Phase 11 update: this remains true after real KPA500/KAT500 read-only polling succeeded. AetherSDR accepts and polls both direct sockets, but applet windows are still not visible. Treat applet activation as a radio-side presence/layout/binary issue, not as proof that the direct PGXL/TGXL serial-backed polling path is broken.

## Phase 5 Emulator Changes

- PGXL/TGXL `info` responses now include explicit `firmware` and `capabilities` fields. These are marked inferred in the protocol field matrix until validated against real devices or AetherSDR UI behaviour.
- Session summary logs include first-command ordering, max inter-command gap, and count of idle gaps over two seconds.
- Strict mock emulation mode can stage mock devices through startup states before reporting connected readiness.

Local TCP smoke after the Phase 5 change shows the deliberate transcript differences:

```text
PGXL info gained: firmware=0.1.0-egb-pgxl capabilities=direct_tcp,status
TGXL info gained: firmware=0.1.0-egb-tgxl capabilities=direct_tcp,status,autotune,ant,manual_tune
TGXL strict startup can report: tuning=1 connection_state=connecting fault=
```

No new AetherSDR command types were observed before these changes; the next macOS run must confirm whether AetherSDR tolerates the enriched `info` lines.

Phase 7 changed mock no-RF status output to report `swr=1.0000` instead of return-loss-style values. The older transcript snippets above remain useful as historical evidence of what AetherSDR accepted during the first stable polling session.

## Next Evidence Needed

1. Capture an AetherSDR session after the richer `info` responses and strict mock startup mode.
2. Confirm whether hidden applets remain hidden despite richer direct responses.
3. If hidden, decide whether the next compatibility path is an AetherSDR patch or a Flex radio API presence proxy.
4. Capture real KPA500/KAT500 read-only serial sessions and compare field semantics against the mock state model.

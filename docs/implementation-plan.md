# Implementation Plan

Status: initial plan after AetherSDR source inspection.

## Phase 1: Protocol Evidence

Completed in this pass:

- Cloned AetherSDR into `research/AetherSDR`.
- Identified PGXL direct TCP implementation.
- Identified TGXL direct TCP implementation.
- Identified manual IP path and lack of direct PGXL/TGXL UDP discovery in inspected code.
- Documented PGXL protocol subset in `docs/protocol-analysis/pgxl.md`.
- Documented TGXL protocol subset in `docs/protocol-analysis/tgxl.md`.
- Documented replay harness design in `docs/architecture/replay-harness-design.md`.
- Documented architecture sketch in `docs/architecture/phase-1-architecture.md`.

Still required before production implementation:

- Capture real TGXL traffic if available.
- Capture real PGXL traffic if available.
- Validate AetherSDR behavior against a fake server.
- Confirm whether SmartSDR for macOS uses the same direct ports and fields.
- Confirm whether Maestro can use manual IP or requires discovery/radio-proxied accessory status.

## Phase 2: Minimal Emulation, No Elecraft Hardware

Goal: make AetherSDR connect manually to fake PGXL/TGXL.

Deliverables:

- Rust workspace scaffold.
- `bridge-core` shared state.
- `pgxl-emulator` TCP server on port `9008`.
- `tgxl-emulator` TCP server on port `9010`.
- Config file with bind IP and ports.
- Transcript logging.
- Replay harness scenarios promoted into integration tests.

Success criteria:

- AetherSDR connects to fake PGXL using manual IP.
- AetherSDR connects to fake TGXL using manual IP.
- PGXL polling at 5 Hz is stable.
- TGXL polling at 1 Hz is stable.
- TGXL relay, autotune, and antenna commands mutate emulator state.
- Unknown/malformed commands are logged and rejected without crashing.

## Phase 3: Elecraft Serial Integration

Goal: map Elecraft hardware to normalized state and command intents.

Deliverables:

- `elecraft-kpa500` serial driver.
- `elecraft-kat500` serial driver.
- Serial reconnect loop with backoff.
- Hardware polling scheduler.
- Command queue with timeouts.
- Device watchdogs and stale-state reporting.

Rules:

- Serial drivers publish normalized state; they do not know PGXL/TGXL protocol.
- Emulators issue normalized intents; they do not know Elecraft serial commands.
- Hardware failures degrade status but do not drop network listeners.

## Phase 4: Windows Daemon Packaging

Deliverables:

- Single executable build for Windows.
- Windows service wrapper.
- Structured logs.
- Rotating file appender.
- Basic diagnostics command.

## Phase 5: Web UI And Security

Deliverables:

- Local web dashboard.
- Config editor.
- COM test actions.
- Raw command tool behind authentication.
- Token auth for web/API.
- Optional TLS support.
- Rate limits and malformed packet limits for raw emulator ports.

## Current MVP Protocol Subset

PGXL:

- `V<version>` server-first handshake.
- `info`.
- `status`.
- `R<seq>|0|state=... peakfwd=... swr=... temp=... id=... vac=... meffa=...`.
- Optional `S0|status ...`.

TGXL:

- `V<version>` server-first handshake.
- `info`.
- `status`.
- `tune relay=<0|1|2> move=<+1|-1>`.
- `autotune`.
- `activate ant=<1|2|3>`.
- `R<seq>|0|operate=... bypass=... tuning=... relayC1=... relayL=... relayC2=... antA=... one_by_three=... fwd=... swr=...`.
- `S0|state ...` after state changes.

## Risk Register

| Risk | Impact | Mitigation |
| --- | --- | --- |
| AetherSDR source comments may encode assumptions from prior reverse engineering | Medium | Validate with real captures and replay tests |
| PGXL native operate/standby direct command unknown | Medium | MVP uses status only; later capture real PGXL or inspect 4O3A tools |
| SmartSDR macOS differs from AetherSDR direct behavior | High | Treat as separate compatibility investigation |
| WAN raw device ports have no native auth | High | Use explicit bind config, firewall/tunnel guidance, and rate limiting |
| Elecraft serial latency conflicts with 5 Hz PGXL polling | Medium | Decouple network polling from serial polling through cached shared state |


# Protocol Replay Harness Design

Status: design only, no implementation yet.

## Purpose

The replay harness exists to validate AetherSDR compatibility before real Elecraft serial integration. It should make protocol assumptions visible and testable.

## Scope

The harness should support:

- TCP server mode for PGXL on `9008`.
- TCP server mode for TGXL on `9010`.
- Scripted server-first `V<version>` handshake.
- Scripted response rules for `info`, `status`, `autotune`, `activate ant=N`, and `tune relay=...`.
- Optional unsolicited `S0|state ...` and `S0|status ...` pushes.
- Full transcript logging with timestamps.
- Golden transcript comparison.

## Non-Goals

- No Elecraft serial support.
- No production daemon/service behavior.
- No security layer except local bind selection.
- No invented protocol fields beyond documented scripts.

## Proposed Directory

```text
tools/protocol-replay/
  README.md
  captures/
    pgxl/
    tgxl/
  scenarios/
    pgxl-minimal.yaml
    tgxl-minimal.yaml
    tgxl-relay-adjust.yaml
    tgxl-autotune.yaml
  transcripts/
```

## Scenario Format

Use a declarative format so captures can become replay tests without code edits:

```yaml
device: tgxl
listen: "0.0.0.0:9010"
version: "1.2.17-egb"
initial_state:
  operate: 0
  bypass: 0
  tuning: 0
  relayC1: 20
  relayL: 35
  relayC2: 20
  antA: 0
  one_by_three: 1
  fwd: 0.0000
  swr: -30.0000
commands:
  info:
    response: "model=TunerGeniusXL serial_num=EGB-TGXL one_by_three=1"
  status:
    response_from_state: true
  "autotune":
    sequence:
      - push: "S0|state tuning=1"
      - delay_ms: 1000
      - push: "S0|state tuning=0"
```

## Harness Phases

1. Passive transcript parser.
   - Read captured lines.
   - Validate framing.
   - Extract commands, responses, status pushes, timings.

2. Scripted replay server.
   - Listen on one port.
   - Emit version line.
   - Match commands.
   - Reply using scenario state.

3. Interactive state emulator.
   - Mutate relay/antenna/tune states.
   - Emit unsolicited pushes on changes.
   - Keep status responses coherent.

4. AetherSDR compatibility run.
   - Start AetherSDR.
   - Configure manual IP to replay harness.
   - Save transcript and screenshot/manual result.

## Required Test Scenarios

PGXL:

- Connect and idle status.
- Transition standby to operate, if direct PGXL command is discovered.
- Transmit telemetry with nonzero `peakfwd`.
- Fault status.

TGXL:

- Connect and idle status.
- Operate/BYPASS/STANDBY status display.
- Relay wheel commands for C1/L/C2.
- Autotune lifecycle.
- 3x1 antenna switch.
- Malformed command resilience.

## Acceptance Criteria

A scenario passes when:

- AetherSDR marks the device connected.
- AetherSDR does not enter a disconnect loop.
- AetherSDR UI controls send the expected commands.
- Harness replies keep UI state coherent.
- Unknown commands do not crash the harness.
- Transcript contains enough information to reproduce failures.


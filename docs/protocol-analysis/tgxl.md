# TGXL Protocol Analysis

Status: source-inspected, not packet-capture validated.

Source inspected:

- `research/AetherSDR` at commit `6d17b3bbda96b836762e7d40758a1fc3e14725f9`
- `src/core/TgxlConnection.h`
- `src/core/TgxlConnection.cpp`
- `src/models/TunerModel.h`
- `src/models/TunerModel.cpp`
- `src/gui/TunerApplet.cpp`
- `src/gui/MainWindow.cpp`
- `src/models/RadioModel.cpp`

## AetherSDR Integration Points

AetherSDR has a direct TCP connection class for Tuner Genius XL:

- Default port: `9010`
- Transport: TCP
- Framing: UTF-8 text, newline-delimited
- Protocol family: `C/R/S/V`
- Client library: Qt `QTcpSocket`

Evidence:

- `TgxlConnection::connectToTgxl(..., port = 9010)` in `TgxlConnection.h:33`
- Poll timer is `1000 ms` in `TgxlConnection.cpp:14`
- Commands are serialized as `C<seq>|<command>\n` in `TgxlConnection.cpp:157`
- Server-first version line is parsed at `TgxlConnection.cpp:79`
- Initial commands are `info` and `status` at `TgxlConnection.cpp:85-86`
- `R...` response parsing starts at `TgxlConnection.cpp:97`
- `S...` status parsing starts at `TgxlConnection.cpp:120`

## Connection Handshake

Observed from AetherSDR source:

```text
Client opens TCP to <host>:9010.
Server must send: V<version>\n
Client sends: C1|info\n
Client sends: C2|status\n
Client marks TGXL connected and starts 1 Hz status polling.
Client sends repeatedly: C<N>|status\n
```

AetherSDR does not mark the TGXL connection ready when TCP connects. It waits for a first line beginning with `V`.

## Message Framing

Client command:

```text
C<seq>|<command>\n
```

Response:

```text
R<seq>|<code>|<body>\n
```

Unsolicited state/status:

```text
S0|state key=value key=value ...\n
S0|status key=value key=value ...\n
```

Version:

```text
V<version>\n
```

## Direct Commands AetherSDR Sends

Mandatory startup and poll commands:

```text
info
status
```

Manual relay control:

```text
tune relay=0 move=1
tune relay=0 move=-1
tune relay=1 move=1
tune relay=1 move=-1
tune relay=2 move=1
tune relay=2 move=-1
```

Relay index mapping from source:

| Relay | Index |
| --- | --- |
| C1 | `0` |
| L | `1` |
| C2 | `2` |

Evidence: `TgxlConnection.cpp:163-168`.

Autotune:

```text
autotune
```

Evidence: `TgxlConnection.cpp:171-174`.

Antenna switch:

```text
activate ant=1
activate ant=2
activate ant=3
```

Evidence: `TunerModel.cpp:124-133`.

## Radio-Proxied Commands AetherSDR Can Send

When a Flex radio reports a TGXL handle, AetherSDR can control operate/bypass through the radio API:

```text
tgxl set handle=<H> mode=1
tgxl set handle=<H> mode=0
tgxl set handle=<H> bypass=1
tgxl set handle=<H> bypass=0
tgxl autotune handle=<H>
```

Evidence: `TunerModel.h:15-17`, `TunerModel.cpp:90-121`.

Important: if direct port `9010` is connected, AetherSDR prefers direct `autotune` over radio-proxied `tgxl autotune handle=<H>`.

## Response Body Format

AetherSDR parses the text after the second pipe in `R<seq>|<code>|<body>` as whitespace-separated `key=value` tokens.

Example valid response shape:

```text
R2|0|operate=0 bypass=0 tuning=0 relayC1=20 relayL=35 relayC2=20 antA=0 one_by_three=1 fwd=0.0000 swr=-30.0000
```

The direct connection emits these response key/value maps as `statusUpdated`.

## Status And State Push Format

AetherSDR distinguishes the object name in `S` lines:

```text
S0|state operate=1 bypass=0 tuning=0 relayC1=20 relayL=35 relayC2=20 antA=0 one_by_three=1
S0|status fwd=0.0000 swr=-30.0000 antA=0
```

`state` maps are routed to `stateUpdated`. `status` maps are routed to `statusUpdated`.

Evidence: `TgxlConnection.cpp:120-149`.

## Fields Consumed By AetherSDR

General TGXL state from radio amplifier status:

| Field | Meaning | Evidence |
| --- | --- | --- |
| `serial_num` | displayed/retained serial | `TunerModel.cpp:35-36` |
| `model` | model name | `TunerModel.cpp:37-38` |
| `operate` | operate state, `1` means on | `TunerModel.cpp:41-43` |
| `bypass` | bypass relay state, `1` means bypass | `TunerModel.cpp:44-46` |
| `tuning` | tuning active, `1` means active | `TunerModel.cpp:47-52` |
| `relayC1` | C1 relay position | `TunerModel.cpp:54-56` |
| `relayC2` | C2 relay position | `TunerModel.cpp:57-59` |
| `relayL` | L relay position | `TunerModel.cpp:60-62` |
| `antA` | selected antenna, zero-indexed in UI | `TunerModel.cpp:64-66` |
| `one_by_three` | enables 3x1 antenna buttons | `TunerModel.cpp:67-69` |
| `ip` | direct TGXL IP for auto-connect | `TunerModel.cpp:70-71` |

Direct TGXL state push fields:

| Field | Meaning | Evidence |
| --- | --- | --- |
| `relayC1` | C1 relay position | `TunerModel.cpp:165-167` |
| `relayL` | L relay position | `TunerModel.cpp:169-171` |
| `relayC2` | C2 relay position | `TunerModel.cpp:173-175` |
| `antA` | selected antenna | `TunerModel.cpp:177-180` |
| `fwd` | forward power in dBm | `TunerModel.cpp:183-191` |
| `swr` | return loss dB, converted to SWR | `TunerModel.cpp:194-198` |

Direct TGXL status poll fields:

| Field | Meaning | Evidence |
| --- | --- | --- |
| `antA` | selected antenna | `TunerModel.cpp:203-212` |
| `fwd` | forward power in dBm | `TunerModel.cpp:217-220` |
| `swr` | return loss dB, converted to SWR | `TunerModel.cpp:223-227` |

## UI State Model

AetherSDR displays TGXL state as:

| `operate` | `bypass` | UI |
| --- | --- | --- |
| `1` | `0` | OPERATE |
| `1` | `1` | BYPASS |
| `0` | any | STANDBY |

Evidence: `TunerApplet.cpp:275-292`.

After `tuning` changes from `1` to `0`, the UI captures SWR for `400 ms` before displaying the tune result. Evidence: `TunerApplet.cpp:25-32`, `TunerApplet.cpp:227-250`.

## Value Conventions Inferred From Source

These are inferred from AetherSDR source, not independently validated against TGXL firmware:

- `fwd` is dBm. AetherSDR converts watts as `10^(dBm / 10) / 1000`.
- `swr` is return loss in dB and is expected to be negative from TGXL.
- `antA` is zero-indexed in received status but `activate ant=N` is one-indexed.
- `one_by_three=1` means show ANT 1/2/3 controls.

## Discovery And Auto-Connect

AetherSDR does not implement direct TGXL UDP discovery in the inspected code.

It connects to TGXL through either:

1. Manual IP in the Peripherals tab.
2. Flex radio amplifier status `model=TunerGeniusXL` with an `ip=<addr>` field.

Evidence:

- Manual TGXL row uses default port `9010` in `RadioSetupDialog.cpp:3979-3980`.
- Auto-connect uses saved `TGXL_ManualIp` and `TGXL_ManualPort` in `MainWindow.cpp:8583-8586`.
- Radio amplifier status routes `model=TunerGeniusXL` to `TunerModel` in `RadioModel.cpp:3733-3792`.
- `TunerModel` stores `ip` as `tgxlIp` in `TunerModel.cpp:70-71`.
- Auto-connect to discovered `tgxlIp` occurs in `MainWindow.cpp:3215-3229`.

## Reconnect Behavior

Direct TGXL connection resets state and calls `QTcpSocket::abort()` before connecting. On socket error it emits `connectionFailed(errorString)`. On disconnect it stops polling and emits `disconnected()`.

There is no independent reconnect backoff loop inside `TgxlConnection`. Reconnects are triggered by manual settings, saved startup settings, or model state changes when an IP appears.

Bridge implication: EGB should support repeated connects and disconnects cleanly and should not assume one long-lived client.

## Packet Captures

No TGXL packet captures exist in this repository yet.

Required capture artifacts before production behavior is finalized:

- `docs/packet-captures/tgxl/connect-handshake.txt`
- `docs/packet-captures/tgxl/status-idle.txt`
- `docs/packet-captures/tgxl/status-operate.txt`
- `docs/packet-captures/tgxl/tune-cycle.txt`
- `docs/packet-captures/tgxl/relay-adjust.txt`
- `docs/packet-captures/tgxl/antenna-switch.txt`

## MVP Emulator Requirements For AetherSDR

Minimum to make AetherSDR show a connected TGXL telemetry source:

1. Listen on TCP `9010`.
2. On accept, send `V<bridge-version>\n`.
3. Accept and parse `C<seq>|info`.
4. Accept and parse repeated `C<seq>|status`.
5. Reply to each command with `R<seq>|0|...`.
6. Include at least `operate`, `bypass`, `tuning`, `relayC1`, `relayL`, `relayC2`, `fwd`, and `swr`.
7. Include `one_by_three=1` and `antA=<0..2>` if exposing antenna switching.
8. Implement `tune relay=<0|1|2> move=<+1|-1>`, `autotune`, and `activate ant=<1..3>` as state-changing commands.
9. Emit `S0|state ...` after state changes so the UI updates without waiting for the next poll.


# PGXL Protocol Analysis

Status: source-inspected, not packet-capture validated.

Source inspected:

- `research/AetherSDR` at commit `6d17b3bbda96b836762e7d40758a1fc3e14725f9`
- `src/core/PgxlConnection.h`
- `src/core/PgxlConnection.cpp`
- `src/gui/MainWindow.cpp`
- `src/gui/AmpApplet.cpp`
- `src/models/RadioModel.cpp`

## AetherSDR Integration Points

AetherSDR has a direct TCP connection class for Power Genius XL:

- Default port: `9008`
- Transport: TCP
- Framing: UTF-8 text, newline-delimited
- Protocol family: same `C/R/S/V` style used by TGXL
- Client library: Qt `QTcpSocket`

Evidence:

- `PgxlConnection::connectToPgxl(..., port = 9008)` in `PgxlConnection.h:26`
- Poll timer is `200 ms` in `PgxlConnection.cpp:14`
- Commands are serialized as `C<seq>|<command>\n` in `PgxlConnection.cpp:137`
- Server-first version line is parsed at `PgxlConnection.cpp:74`
- Initial commands are `info` and `status` at `PgxlConnection.cpp:79-80`
- `R...` response parsing starts at `PgxlConnection.cpp:89`
- `S...` unsolicited status parsing starts at `PgxlConnection.cpp:110`

## Connection Handshake

Observed from AetherSDR source:

```text
Client opens TCP to <host>:9008.
Server must send: V<version>\n
Client sends: C1|info\n
Client sends: C2|status\n
Client marks PGXL connected and starts 5 Hz status polling.
Client sends repeatedly: C<N>|status\n
```

AetherSDR does not mark the PGXL connection ready when TCP connects. It waits for the first non-empty line beginning with `V`. A bridge emulator must therefore send the version line immediately after accept.

## Message Framing

All messages are line-oriented and terminated by LF. AetherSDR trims each line, so CRLF should also be accepted.

Client command:

```text
C<seq>|<command>\n
```

Response:

```text
R<seq>|<code>|<body>\n
```

Unsolicited status:

```text
S<seq-or-0>|<object> key=value key=value ...\n
```

Version:

```text
V<version>\n
```

## Commands AetherSDR Sends Directly

Mandatory direct PGXL commands:

```text
info
status
```

`status` is sent every 200 ms after the version handshake.

Operate/standby is not sent over the direct PGXL TCP path in current AetherSDR. The AMP applet emits `operateToggled`, but `MainWindow` routes that to the Flex radio amplifier API:

```text
amplifier set <handle> operate=<0|1>
```

Evidence: `MainWindow.cpp:3304-3308`.

For Elecraft Genius Bridge, MVP PGXL direct emulation only needs to satisfy telemetry polling. If AetherSDR is connected without a Flex radio amplifier handle, direct PGXL operate control may not be reachable from current UI code.

## Response Body Format

AetherSDR parses the text after the second pipe in `R<seq>|<code>|<body>` as whitespace-separated `key=value` tokens.

Example valid response shape:

```text
R2|0|state=IDLE peakfwd=30.0 swr=-30.0 temp=32 id=0.0 vac=230 meffa=OK
```

Unknown keys are ignored.

Phase 5 compatibility enrichment adds these direct `info` keys in EGB:

```text
firmware=<bridge-version>
capabilities=direct_tcp,status
```

These fields are TODO/UNVERIFIED and exist to make transcript differences explicit while investigating applet activation. They are not yet known real PGXL fields.

## Status Push Format

AetherSDR parses an `S` line by skipping an object prefix and consuming `key=value` tokens after the last space before the first `=`.

Example valid status push shape:

```text
S0|status state=IDLE peakfwd=30.0 swr=-30.0 temp=32 id=0.0 vac=230 meffa=OK
```

The object name is not used for PGXL direct status updates.

## Fields Consumed By AetherSDR

Direct PGXL telemetry consumed by `MainWindow`:

| Field | Meaning in AetherSDR | Evidence |
| --- | --- | --- |
| `temp` | PA temperature in deg C | `MainWindow.cpp:3245-3246` |
| `id` | drain current in amps | `MainWindow.cpp:3247-3248` |
| `vac` | mains voltage | `MainWindow.cpp:3249-3250` |
| `state` | amp state and TX state | `MainWindow.cpp:3251-3252` |
| `meffa` | displayed MEffA status | `MainWindow.cpp:3253-3254` |
| `peakfwd` | PGXL peak forward power | `MainWindow.cpp:3259-3262` |
| `swr` | interpreted as return loss dB, converted to SWR | `MainWindow.cpp:3264-3269` |

The AMP applet treats these states as operating:

```text
IDLE
OPERATE
TRANSMIT*
```

and these as non-operating:

```text
STANDBY
POWERUP
FAULT
```

Evidence: `AmpApplet.cpp:115-120`.

## Value Conventions Inferred From Source

These are inferred from AetherSDR source, not independently validated against PGXL firmware:

- `peakfwd` is dBm. AetherSDR converts watts as `10^((dBm - 30) / 10)`.
- `swr` is treated as return loss in dB. AetherSDR takes absolute value and converts return loss to SWR.
- `state` beginning with `TRANSMIT` forces TX meter mode.

Do not freeze these conventions as final protocol truth until validated with real PGXL or trusted captures.

## Discovery And Auto-Connect

AetherSDR does not implement direct PGXL UDP discovery in the inspected code.

It connects to PGXL through either:

1. Manual IP in the Peripherals tab.
2. Flex radio amplifier status providing `model=PowerGeniusXL` and `ip=<addr>`.

Evidence:

- Manual PGXL row uses default port `9008` in `RadioSetupDialog.cpp:4008-4009`.
- Auto-connect uses saved `PGXL_ManualIp` and `PGXL_ManualPort` in `MainWindow.cpp:8588-8591`.
- Radio amplifier status detects non-TGXL amplifier models and stores `ip` as `m_ampIp` in `RadioModel.cpp:3796-3805`.
- Auto-connect to detected amplifier IP occurs in `MainWindow.cpp:3234-3237`.

## Reconnect Behavior

Direct PGXL connection has minimal reconnect behavior in the inspected class:

- On disconnect, polling stops and `disconnected()` is emitted.
- On manual connect while already connected, it calls `disconnect()`.
- It does not call `abort()` before reconnect, unlike TGXL.
- Reconnects are triggered by UI/manual settings or radio amplifier presence changes, not by an internal backoff loop.

Bridge implication: the emulator should tolerate repeated TCP connects and clean disconnects. For WAN use, EGB should implement its own robust server-side client lifecycle instead of relying on AetherSDR to aggressively retry.

## Packet Captures

No PGXL packet captures exist in this repository yet.

Required capture artifacts before production behavior is finalized:

- `docs/packet-captures/pgxl/connect-handshake.txt`
- `docs/packet-captures/pgxl/status-idle.txt`
- `docs/packet-captures/pgxl/status-transmit.txt`
- `docs/packet-captures/pgxl/operate-standby.txt`, if native PGXL TCP operate commands are discovered

## MVP Emulator Requirements For AetherSDR

Minimum to make AetherSDR show a connected PGXL telemetry source:

1. Listen on TCP `9008`.
2. On accept, send `V<bridge-version>\n`.
3. Accept and parse `C<seq>|info`.
4. Accept and parse repeated `C<seq>|status`.
5. Reply to each command with `R<seq>|0|...`.
6. Include at least `state`, `peakfwd`, `swr`, `temp`, `id`, `vac`, and `meffa` in status bodies.
7. Do not block or close on unknown commands; return a structured error or empty success until real behavior is known.

## Phase 5 Applet Visibility Finding

Successful direct PGXL connection and polling do not by themselves make the AetherSDR AMP applet tray button visible in the inspected build. The applet visibility path is driven by `RadioModel::amplifierChanged`, which depends on Flex radio API `amplifier` status records. See `docs/analysis/aethersdr-session-sequence.md`.

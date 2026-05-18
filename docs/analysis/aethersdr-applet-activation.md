# AetherSDR Applet Activation Investigation

Status: source-inspected against local `research/AetherSDR` checkout at commit `6d17b3bbda96b836762e7d40758a1fc3e14725f9`.

## Exact Files And Classes

| Area | File | Class / Function | Role |
| --- | --- | --- | --- |
| Applet creation | `src/gui/AppletPanel.cpp` | `AppletPanel` constructor | Creates `TUN` and `AMP` applets hidden by default. |
| Applet visibility | `src/gui/AppletPanel.cpp` | `AppletPanel::setTunerVisible`, `AppletPanel::setAmpVisible` | Shows/hides tray buttons through `applyConditionalPresence`. |
| Wiring | `src/gui/MainWindow.cpp` | setup block around `TunerModel::presenceChanged` and `RadioModel::amplifierChanged` | Connects model presence signals to applet visibility. |
| TGXL model | `src/models/TunerModel.h/.cpp` | `TunerModel::isPresent`, `setHandle`, `setDirectConnection`, `applyStatus` | Determines tuner presence from radio handle or direct TCP connection. |
| PGXL model | `src/models/RadioModel.h/.cpp` | `m_hasAmplifier`, `amplifierChanged`, `ampTelemetryUpdated` | Determines power amplifier presence from radio API `amplifier` status. |
| Flex status parse | `src/core/CommandParser.cpp` | `CommandParser::parseLine` | Parses SmartSDR `S<handle>|<object> key=value` status lines. |
| Radio status dispatch | `src/core/RadioConnection.cpp` | `RadioConnection::processLine` | Emits parsed status to `RadioModel`. |
| Direct PGXL socket | `src/core/PgxlConnection.cpp` | `PgxlConnection::processLine` | Accepts `V`, sends `info/status`, emits telemetry only. |
| Direct TGXL socket | `src/core/TgxlConnection.cpp` | `TgxlConnection::processLine` | Accepts `V`, sends `info/status`, emits connection, state, and telemetry. |

## Applet Creation Defaults

`AppletPanel` creates both accessory applets at startup, but their tray buttons start hidden:

```text
TUN: makeEntry("TUN", "Tuner", ..., defaultOn=false); button hidden
AMP: makeEntry("AMP", "Amplifier", ..., defaultOn=false); button hidden
```

Only `MainWindow` later calls `setTunerVisible(true)` or `setAmpVisible(true)`.

## AMP / PGXL Activation

The AMP applet visibility path is:

```text
Flex radio API status
  -> RadioConnection::processLine
  -> CommandParser::parseLine
  -> RadioModel::onStatusReceived(object="amplifier <handle>", kvs)
  -> non-empty model where model != TunerGeniusXL
  -> m_hasAmplifier = true
  -> emit amplifierChanged(true)
  -> MainWindow lambda
  -> AppletPanel::setAmpVisible(true)
```

Required radio-side status shape:

```text
S<radio-handle>|amplifier <handle> model=PowerGeniusXL ip=<addr> state=<STATE> ...
```

The exact model does not have to be `PowerGeniusXL` in this AetherSDR code; any non-empty `model` other than `TunerGeniusXL` is treated as a power amplifier. For PGXL-like behaviour, use `model=PowerGeniusXL`.

Important consequence: direct PGXL TCP connection alone does not call `amplifierChanged(true)` and therefore does not show the AMP applet. `PgxlConnection::connected` logs the direct connection and its `statusUpdated` signal feeds telemetry, but visibility remains gated by `RadioModel::hasAmplifier()`.

Operate/standby control also depends on a radio-side amplifier handle:

```text
amplifier set <handle> operate=<0|1>
```

The direct PGXL socket path is telemetry-only in the inspected AetherSDR code.

## TUN / TGXL Activation

The TUN applet has two activation paths.

Radio-side path:

```text
Flex radio API status
  -> object="amplifier <handle>"
  -> model=TunerGeniusXL
  -> RadioModel routes status to TunerModel
  -> TunerModel::setHandle(<handle>)
  -> TunerModel::presenceChanged(true)
  -> AppletPanel::setTunerVisible(true)
```

Required radio-side status shape:

```text
S<radio-handle>|amplifier <handle> model=TunerGeniusXL ip=<addr> operate=<0|1> bypass=<0|1> tuning=<0|1> relayC1=<n> relayL=<n> relayC2=<n> antA=<0..2> one_by_three=1 ...
```

Direct TCP fallback path:

```text
manual/saved TGXL IP
  -> MainWindow m_tgxlConn.connectToTgxl(...)
  -> TgxlConnection receives server-first V line
  -> emits connected()
  -> TunerModel::setDirectConnection handler sets m_directPresence=true
  -> TunerModel::presenceChanged(true)
  -> AppletPanel::setTunerVisible(true)
```

This is explicitly supported by the inspected source. `TunerModel::isPresent()` returns true when either a radio-side handle exists or direct presence is true.

If a real AetherSDR build connects to TGXL direct TCP but does not show `TUN`, likely causes are:

- the inspected local source does not match the binary under test,
- the direct `TgxlConnection::connected` signal is not reaching `TunerModel::setDirectConnection`,
- the applet tray is filtered/collapsed by user layout state,
- or the connection was made through a different object than `MainWindow::m_tgxlConn`.

## SmartSDR API Message Handling

AetherSDR receives normal radio API lines through `RadioConnection`. `CommandParser::parseLine` handles `S` lines by splitting the object name from key/value fields at the last space before the first `=`.

Examples:

```text
S23A59BDF|amplifier 0x42000001 model=PowerGeniusXL ip=192.168.1.50 state=STANDBY
S23A59BDF|amplifier 0x42000002 model=TunerGeniusXL ip=192.168.1.50 operate=0 bypass=0 tuning=0 relayC1=20 relayL=35 relayC2=20 antA=0 one_by_three=1
S23A59BDF|atu 0 status=TUNE_SUCCESSFUL atu_enabled=1
```

AetherSDR subscribes to the relevant radio objects during connect:

```text
sub atu all
sub amplifier all
sub meter all
```

The `amplifier` object is the critical one for external PGXL/TGXL applet presence.

## Dependency Matrix

| Device UI | Direct Socket Alone | Radio-side `amplifier` Status | Both Together | Notes |
| --- | --- | --- | --- | --- |
| PGXL connected indicator | Yes, direct object connects and polls | Yes, can auto-connect by `ip` | Best | Direct connection alone does not show AMP applet. |
| AMP applet visibility | No | Yes | Yes | Requires `RadioModel::amplifierChanged(true)`. |
| AMP telemetry | Yes, if applet already visible | Yes fallback | Best | Direct telemetry preferred when connected. |
| AMP operate/standby | No | Yes | Yes | UI sends `amplifier set <handle> operate=<0|1>`. |
| TGXL connected indicator | Yes | Yes | Best | Direct fallback exists. |
| TUN applet visibility | Yes in inspected source | Yes | Best | If hidden in binary, verify source/binary mismatch first. |
| TGXL relay/manual tune | Yes | No | Yes | Direct socket required for `tune relay=...`. |
| TGXL operate/bypass | No direct command in UI | Yes | Yes | UI uses radio-side `tgxl set handle=...`. |
| TGXL autotune | Yes when direct connected | Yes fallback | Best | Direct path preferred by `TunerModel::autoTune()`. |

## Integration Options

### Option A: Direct PGXL/TGXL Only

Feasibility: already mostly implemented.

Risk: low for transport and polling, but incomplete AetherSDR UI integration.

Required work: continue protocol fidelity, hardware validation, docs.

SmartLink compatibility: good, because it does not touch radio control path.

Remote operation: works for direct ports if reachable, but raw WAN exposure remains unsafe.

Requires modifying AetherSDR: not for TGXL direct visibility in inspected source; yes or degraded UX for PGXL applet/operate visibility.

Assessment: acceptable for protocol validation and TGXL direct testing, insufficient for full PGXL applet behaviour.

### Option B: Flex API Client Mode, Track Only

Feasibility: moderate. EGB connects to the Flex radio as an additional API client and observes frequency/PTT/state.

Risk: low to medium. It does not modify AetherSDR traffic but adds another radio client.

Required work: implement Flex TCP API client, discovery/config, status subscriptions, command mapping.

SmartLink compatibility: difficult unless EGB also implements SmartLink authentication/cloud access; straightforward on LAN/VPN.

Remote operation: works when EGB is at the radio site on LAN with the radio.

Requires modifying AetherSDR: no.

Assessment: useful for frequency/state sync and safety, but it cannot make AetherSDR show PGXL AMP applets because AetherSDR still does not receive injected `amplifier` presence records.

### Option C: Flex API Proxy/Interceptor With Minimal Injection

Feasibility: high on LAN/direct TCP, complex with SmartLink.

Risk: medium to high. It becomes part of the primary radio control path and must be transparent, robust, and protocol-safe.

Required work: transparent TCP proxy, line parser, response sequence preservation, status injection, connection lifecycle, TLS/cloud constraints, extensive replay tests.

SmartLink compatibility: poor unless the proxy can sit after SmartLink resolution as a local endpoint, or EGB implements the SmartLink client side. A transparent LAN TCP proxy is not a SmartLink cloud proxy.

Remote operation: works for LAN/VPN/direct public radio TCP where AetherSDR can be pointed at the proxy instead of the radio.

Requires modifying AetherSDR: no, if user can configure AetherSDR to connect to the proxy endpoint.

Assessment: cleanest no-AetherSDR-patch path for full PGXL/TGXL applet activation when the real radio does not advertise these devices.

### Option D: Configure Flex Radio To Advertise PGXL/TGXL

Feasibility: unknown; depends on Flex firmware/settings and whether it can register external Genius devices without the real hardware.

Risk: low if supported natively, otherwise impossible.

Required work: investigate Flex radio configuration, SmartSDR docs, and real radio status output.

SmartLink compatibility: best if the radio itself emits the records.

Remote operation: best; AetherSDR gets normal radio-side status through any supported connection path.

Requires modifying AetherSDR: no.

Assessment: preferred if possible, but not yet evidenced.

## Recommended Strategy

1. Continue EGB as direct PGXL/TGXL bridge for device protocol and Elecraft hardware validation.
2. For TGXL applet visibility, first verify the exact AetherSDR binary against the inspected source because direct TCP should be enough in this revision.
3. For PGXL applet visibility and operate/standby controls, investigate Flex radio-side configuration first.
4. If the radio cannot advertise synthetic PGXL/TGXL presence, design optional Flex API proxy mode as a separate Phase 7 feature. Keep it disabled by default and keep direct PGXL/TGXL sockets on `9008/9010`.

## Phase 16 Confirmation

The exact conditional UI code confirms that `Applet_TUN` and `Applet_AMP` settings restore only after presence is true. They are not manual visibility overrides.

The inspected source includes the `0.9.5` release-note entry `TGXL detected via direct TCP only (#2250, chrisb1964)`, so the tested macOS binary should be checked against `0.9.5` or newer before concluding direct TGXL applet activation is broken.

PGXL direct TCP still does not provide AMP presence in this source. The direct PGXL connection feeds `AmpApplet` telemetry only after a radio-side `amplifier` record has already made the AMP applet visible.

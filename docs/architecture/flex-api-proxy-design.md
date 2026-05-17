# Flex API Proxy Design

Status: design only. Do not implement until applet activation requirements are validated with the target AetherSDR binary and Flex radio configuration.

## Problem

AetherSDR shows PGXL/TGXL applets from radio model state. PGXL AMP visibility, and some TGXL controls, depend on SmartSDR/Flex API `amplifier` status records. Direct PGXL/TGXL TCP sockets provide telemetry/control transport but do not create PGXL radio-side presence records.

## Goal

Add an optional mode where AetherSDR connects to EGB as if it were the Flex radio TCP API endpoint. EGB forwards all traffic to the real Flex radio and injects the smallest validated set of accessory status records needed to make AetherSDR instantiate and bind PGXL/TGXL UI state.

Direct PGXL/TGXL emulators remain independent on ports `9008` and `9010`.

## Non-Goals

- No SmartLink cloud reimplementation in the first proxy design.
- No generic Flex server emulation.
- No mutation of unrelated radio status.
- No broad protocol rewriting.
- No authentication bypass.

## Proposed Topology

```text
AetherSDR
   |
   | Flex TCP API
   v
EGB Flex API proxy
   |
   | forwarded Flex TCP API
   v
Real Flex radio

AetherSDR ---> EGB PGXL direct TCP :9008
AetherSDR ---> EGB TGXL direct TCP :9010
EGB      ---> KPA500/KAT500 serial
```

## Transparent Proxy Requirements

- Accept one AetherSDR TCP client on a configured local/LAN port.
- Connect to the real Flex radio TCP API endpoint.
- Forward client-to-radio bytes without modification by default.
- Forward radio-to-client bytes without modification by default.
- Preserve all line ordering except explicitly injected accessory status.
- Preserve response sequence semantics; never synthesize `R<seq>` replies unless the proxy owns the command.
- Detect `H<client-handle>` and radio status bursts.
- Detect subscription completion for `sub amplifier all` if needed.
- Inject only validated `S<handle>|amplifier ...` status lines.
- Log injected lines distinctly from forwarded lines.

## Minimal Injection Candidates

PGXL presence:

```text
S<radio-handle>|amplifier 0xEGB50001 model=PowerGeniusXL ip=<egb-ip> state=STANDBY serial_num=EGB-PGXL
```

TGXL presence:

```text
S<radio-handle>|amplifier 0xEGB50002 model=TunerGeniusXL ip=<egb-ip> operate=0 bypass=0 tuning=0 relayC1=20 relayL=35 relayC2=20 antA=0 one_by_three=1 serial_num=EGB-TGXL
```

These are candidate shapes only. They must be validated against real AetherSDR sessions before implementation.

## Command Interception

If applets send radio-side commands for synthetic handles, the proxy must decide whether to:

1. Intercept and translate:
   - `amplifier set 0xEGB50001 operate=<0|1>` -> KPA500 desired operate/standby.
   - `tgxl set handle=0xEGB50002 mode=<0|1>` -> KAT500 operate/bypass semantics if validated.
   - `tgxl set handle=0xEGB50002 bypass=<0|1>` -> KAT500 bypass if validated.
   - `tgxl autotune handle=0xEGB50002` -> KAT500 autotune if safe.
2. Forward all other commands unchanged to the radio.
3. Return a valid Flex API response only for intercepted commands, preserving the client sequence number.

This is more invasive than status injection and should be implemented after passive injection is proven.

## State Source

Injected status should derive from EGB shared state:

- KPA500 serial polling updates amp state.
- KAT500 serial polling updates tuner state.
- Direct PGXL/TGXL socket commands update desired state.
- Proxy-injected status reflects actual/degraded state, not hardcoded healthy state.

## SmartLink Implications

LAN radio access:

- Feasible. AetherSDR connects to proxy IP/port instead of the radio IP/port.

VPN/Tailscale:

- Feasible if AetherSDR can route to the proxy and the proxy can route to the radio.

Direct public IP:

- Technically feasible but not recommended without security hardening. The proxy would expose the Flex API control plane.

SmartLink cloud:

- Not supported by a simple transparent LAN TCP proxy. SmartLink involves cloud discovery/authentication and the client may establish tunnels/endpoints that are not a plain user-configurable radio TCP socket.
- Possible future approaches:
  - EGB runs at the radio site and AetherSDR connects to EGB over VPN/Tailscale instead of SmartLink.
  - EGB implements a SmartLink-aware client/proxy, which is a major separate project.
  - AetherSDR is patched to accept external accessory presence from direct PGXL/TGXL sockets, avoiding Flex API proxying.

## Failure Handling

- If proxy loses the radio connection, close or degrade the client connection predictably.
- If serial device state is stale, inject degraded/fault status rather than healthy status.
- If injected handles conflict with real radio handles, disable injection and log an error.
- If AetherSDR sends unsupported commands for synthetic handles, return a clear error response and increment counters.

## Validation Plan

1. Capture a real radio session with actual PGXL/TGXL, if available, to confirm status field shape.
2. Build replay fixtures for radio-side `amplifier` presence.
3. Implement a passive proxy that forwards traffic and logs status only.
4. Add optional injection in mock mode.
5. Validate AetherSDR applet visibility.
6. Validate applet controls with dry-run serial mode before any RF-risk command.

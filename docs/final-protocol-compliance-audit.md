# Final Protocol Compliance Audit

Phase 65 reviewed EGB against the official TGXL/PGXL-family protocols, the
AetherSDR TGXL wiki, the local AetherSDR source tree, and live evidence from the
latest band-follow/AetherSDR runs.

## Summary

| Area | Status | Notes |
| --- | --- | --- |
| KPA500 serial telemetry | Implemented | `^OS`, `^WS`, `^TM`, `^VI`, and `^FL` feed PGXL/Flex state. Healthy KPA state must never produce PGXL `UNKNOWN`. |
| KPA500 control | Partial, guarded | Standby maps to `^OS0;` when enabled. Operate maps to `^OS1;` only with RF-risk enablement. Band follow maps Flex band to `^BNnn;` in the explicit test profile. |
| KAT500 serial telemetry | Implemented | `AN`, `BYP`, `MD`, `TP`, `FLT`, `VSWR`, `VFWD`, and unsolicited bursts are routed through the shared command queue. |
| KAT500 control | Partial, guarded | `F <kHz>;` frequency follow and `T;` tune are implemented. Bypass/antenna switching remain disabled unless separately enabled. |
| TGXL direct TCP | Mostly implemented | Greeting, `info`, `status`, `flexradio`, `catradio`, `setup`, `autotune`, and safe blocked responses are implemented. Status includes the official major fields used by AetherSDR. |
| PGXL direct TCP | Implemented for AetherSDR telemetry | Greeting, `info`, and `status` are stable. Direct PGXL operate commands are not observed from AetherSDR; AetherSDR operates via Flex `amplifier set`. |
| Flex amplifier registration | Implemented with compatibility profile | EGB sends amplifier create, meter create, optional interlock create, keepalive, subscriptions, and ping. Latest evidence has no amplifier removal or handle churn. |
| Flex meter publication | Missing/unknown | Meter objects are created, but no verified client-side Flex meter value publication command is implemented. SmartSDR PGXL live data remains partial. |
| SmartSDR TGXL/tuner | Missing/unsupported | No verified public Flex-side external TGXL/tuner registration API was found in the reviewed code/docs. Direct TGXL TCP is an AetherSDR path. |
| AetherSDR PGXL auto-open | Fixed by config alignment | AetherSDR opens PGXL from the Flex amplifier `ip`. EGB must advertise the same reachable address that the PGXL listener is bound to. |

## TGXL Direct TCP

The AetherSDR wiki states that TGXL exposes a direct TCP connection on port
9010 and uses `C<seq>|command`, `R<seq>|code|body`, and `S0|state ...` style
messages. EGB follows that framing and keeps the direct TGXL path stable for
AetherSDR.

Implemented or intentionally handled:

- `V...` greeting.
- `info`.
- `status` with `fwd`, `peak`, `max`, `swr`, `pttA`, `pttB`, `bandA`, `bandB`,
  `modeA`, `modeB`, `flexA`, `flexB`, `freqA`, `freqB`, `bypassA`,
  `bypassB`, `bypassRxA`, `bypassRxB`, `antA`, `antB`, `state`, `active`,
  `tuning`, `bypass`, `ag`, `relayC1`, `relayL`, and `relayC2`.
- `autotune` maps to KAT500 `T;` when effective controls allow it.
- Flex frequency changes update TGXL `freqA`/`bandA`.
- Unsupported relay move and destructive configuration writes are rejected
  safely rather than being silently accepted.

Remaining TGXL limitations:

- EGB emulates a TGXL surface over a KAT500. KAT500 does not expose full TGXL
  relay-control semantics, so manual relay movement is intentionally unsupported.
- SmartSDR TGXL visibility is not provided by the direct TCP protocol.

## PGXL Direct TCP

EGB implements the direct PGXL telemetry surface that AetherSDR polls after the
direct socket is open:

- `V...` greeting.
- `info`.
- `status` derived from live KPA500 state.
- `state=STANDBY|OPERATE|FAULT` from `^OS` and `^FL`.
- `temp` from `^TM`.
- `id` from `^VI` current where available.
- `peakfwd` and return-loss/SWR fields from `^WS`.
- `meffa=OK` when KPA500 fault is `00`.

Important invariant:

- Once KPA500 has a recent successful poll, PGXL direct status must not emit
  `state=UNKNOWN` or zero/default telemetry unless the real KPA telemetry is
  actually unknown or zero.

## Flex Amplifier Registration

EGB’s intended sequence is:

1. Wait for Flex `H<handle>`.
2. Send `amplifier create ip=<advertised> port=9008 model=PowerGeniusXL serial_num=<serial> ant=ANT1:PORTA,ANT2:PORTB ...`.
3. Create AMP meters `FWD`, `RL`, `DRV`, `ID`, and `TEMP`.
4. Create AMP interlock unless disabled for a test profile.
5. Send `keepalive enable`.
6. Subscribe to `amplifier`, `slice`, and `tx` status.
7. Ping periodically.
8. Reannounce state changes without recreating objects.

The live failure that Phase 65 isolated was not a Flex registration syntax
failure. It was a reachability mismatch: Flex/Aether received an amplifier
status advertising `192.168.0.189` while the PGXL direct server was bound to
`127.0.0.1`.

## Compatibility Hacks

`aethersdr_force_direct` remains the locked AetherSDR compatibility profile. It
is not the strict official profile, but it is the evidence-backed profile that
keeps AetherSDR’s widgets stable.

Retained hacks:

- AetherSDR-specific amplifier create/status fields in the compatibility
  profile.
- Startup subscription burst for AetherSDR connection timing.

Removed or disabled by default:

- `pgxl_connect_assist` / synthetic `amplifier set operate=1` lifecycle hack.

## Production Rules

- Advertised PGXL IP must be reachable from AetherSDR.
- For same-host AetherSDR with loopback listeners, advertise `127.0.0.1`.
- For LAN AetherSDR, bind EGB to the LAN IP and advertise the same LAN IP.
- Do not recreate Flex amplifier objects for telemetry changes.
- Do not change KPA/KAT serial polling while control commands are in flight;
  use the existing command queues.

## Phase 65 Live Verification

Short live run:

`target\release\egb.exe band-follow-test --config config.aethersdr-kpa-band-follow-test.yaml --duration-minutes 1`

Result:

- Evidence ZIP: `diagnostics\runs\20260524-001114-band-follow-test.zip`.
- PGXL accepted about 0.2 seconds after listener readiness with
  `ip=127.0.0.1`.
- KPA500 polls: 58 successes, 0 failures.
- KAT500 polls: 58 successes, 0 failures.
- KAT frequency follow: exact confirmation true.
- KPA band follow: exact confirmation true.
- Amplifier removed count: 0.
- Handle churn: none.

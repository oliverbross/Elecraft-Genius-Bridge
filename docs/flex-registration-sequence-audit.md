# Flex Registration Sequence Audit

Status: Phase 49 audit.

## Intended Sequence

The operational Flex amplifier registration sequence is:

1. Start EGB with a current binary. Print executable path, working directory, embedded git commit, build timestamp, and config path.
2. Validate advertised PGXL IP. It must be the Windows bridge LAN IP for LAN/remote clients, not `127.0.0.1`, unless the Flex radio and client path are both local-only.
3. Open KPA500 COM port and complete read-only preflight:
   - `^OS;`
   - `^WS;`
   - `^TM;`
   - `^VI;`
   - `^FL;`
4. If KPA preflight fails, do not create the Flex amplifier object and do not start a healthy PGXL lifecycle.
5. Connect to Flex TCP API port 4992 and wait for `H<client-handle>`.
6. Send one amplifier create command. For strict Flex/SmartSDR protocol audit this is:
   - `amplifier create ip=<egb-lan-ip> port=9008 model=PowerGeniusXL serial_num=<serial> ant=ANT1:PORTA,ANT2:PORTB`
   For AetherSDR operational testing, `aethersdr_minimal` adds only the suspected trigger field:
   - `state=<live-kpa-state>`
   This field is explicitly an AetherSDR compatibility profile, not the strict official create path.
7. Wait briefly for the Flex radio to accept and/or broadcast the amplifier object handle.
8. If no handle/status arrives after `amplifier create` is accepted, continue registration using the documented serial/name fallback.
9. Create meters once per Flex session, after the amplifier handle is observed or after the create-accepted fallback:
   - `meter create name=FWD type=AMP ...`
   - `meter create name=RL type=AMP ...`
   - `meter create name=DRV type=AMP ...`
   - `meter create name=ID type=AMP ...`
   - `meter create name=TEMP type=AMP ...`
10. Create interlock once per Flex session, after the amplifier handle is observed or after the create-accepted fallback:
   - `interlock create type=AMP valid_antennas=<configured-antennas> name=PG-XL serial=<serial>`
11. Enable keepalive and subscriptions:
   - `keepalive enable`
   - `sub amplifier all`
   - `sub slice all`
   - `sub tx all`
12. Ping periodically.
13. Do not recreate amplifier/meter/interlock objects unless the Flex TCP session actually reconnects.
14. Do not send `amplifier set <handle> operate=1` unless it is a real user operate request and RF-risk control is explicitly enabled.

## Latest Failed Live Run Comparison

The latest failed evidence showed these violations:

- Runtime binary was old: running commit `f831202`, expected Phase 48 commit `1cd40c9`.
- KPA500 COM21 failed with `Access is denied`, so the first poll never completed.
- PGXL/Flex startup continued despite unavailable KPA telemetry, allowing `UNKNOWN`/zero state to reach clients.
- Flex registration used `ip=127.0.0.1`, which is not reachable from a LAN macOS AetherSDR client or the Flex radio path.
- `pgxl_connect_assist` sent `amplifier set <handle> operate=1`, which had already been observed as rejected (`500000A7`).
- Flex later sent `amplifier <handle> removed`, causing AMP widget flapping.

## Phase 49 Corrections

- Development-checkout runs now refuse to start if the embedded binary commit differs from repository HEAD.
- Startup prints executable path, working directory, git commit, build timestamp, and config path.
- KPA500 preflight now blocks PGXL/Flex amplifier startup when the port is locked or required read-only commands fail.
- KAT500 preflight now blocks TGXL startup when the port is locked or read-only polling fails.
- Operational/evidence runs reject loopback PGXL advertised IP when the Flex radio path is LAN.
- Operational/evidence runs reject `pgxl_connect_assist=true`.
- Operational/evidence runs reject old lab amplifier profiles that add broad or unsafe non-standard fields to `amplifier create`. `aethersdr_minimal` is allowed because it is the explicit AetherSDR compatibility profile and keeps connect-assist disabled.
- Meter, interlock, keepalive, and subscription commands are now sent after the amplifier handle is observed, or after a short create-accepted fallback when the radio accepts the object but does not broadcast a handle/status. This avoids blocking registration forever while preserving the documented command sequence.
- If Flex sends `amplifier <handle> removed`, EGB marks the lifecycle failed and stops re-registering until restart instead of entering a create/remove loop.
- Operational configs now advertise `192.168.0.189` and keep connect-assist disabled.

## Remaining Live Proof Needed

After rebuilding the current commit, run a short evidence test with KPA500 Remote closed:

```powershell
.\target\release\egb.exe evidence-test --config .\config.aethersdr-compatible-operational.yaml --duration-minutes 5
```

Expected safe outcomes:

- If COM21 is locked, EGB exits before Flex amplifier create with `KPA500_PORT_LOCKED_OR_UNAVAILABLE`.
- If COM21 is free, KPA preflight passes before amplifier create.
- Flex amplifier create uses `ip=192.168.0.189` and only the `aethersdr_minimal` `state=<live>` compatibility field.
- `interlock-registration-audit.md` should show whether `reason=AMP:PG-XL` still has an empty `amplifier=` field.
- No `amplifier set <handle> operate=1` is sent unless an RF-risk user command is received and explicitly enabled.

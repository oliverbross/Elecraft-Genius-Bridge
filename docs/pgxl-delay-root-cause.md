# PGXL Delay Root Cause

Latest Phase 62 evidence proves the PGXL TCP listener is not the source of the 38-40 second delay.

Observed:

- PGXL listener ready: `1779540036424`
- PGXL first accept: `1779540079626`
- Delay from listener ready: about `43.2s`
- Delay after TGXL accept: about `38.251s`
- Before PGXL accept: `reannounce_count=16`, `sub_amp_all_count=23`
- `egb pgxl-self-probe --host 127.0.0.1 --port 9008` connects immediately and receives valid `info` / `status`.
- Self-probe status reported live telemetry, for example `state=OPERATE temp=39.0 meffa=OK`.

Conclusion: the EGB PGXL server is ready and responds immediately. The delayed PGXL open is upstream of the EGB listener. In the absence of an earlier accepted socket, classification is:

`A: AetherSDR did not attempt/complete TCP 9008 before the delay.`

Phase 65 source/evidence comparison found the practical cause for the local
Windows test profile:

- AetherSDR auto-opens PGXL from the Flex amplifier `ip` field.
- EGB advertised `192.168.0.189`.
- EGB was bound to `127.0.0.1`.
- The successful accepted PGXL session came later from `127.0.0.1`, consistent
  with AetherSDR's manual/local peripheral path rather than the immediate Flex
  amplifier trigger.

Rust application code cannot observe raw TCP SYN packets before `accept()`. If SYN-level proof is needed, capture with Wireshark or Windows `pktmon`. The EGB-side proof is the combination of listener-ready timestamp, no early accepted PGXL session, immediate local self-probe success, and the advertised-IP/listener-bind mismatch.

Do not spend further time changing PGXL listener readiness or handshake timing unless future evidence shows an early accepted socket with a failed handshake. Use a consistent address instead:

- local AetherSDR: `server.bind_ip=127.0.0.1` and `force_advertised_pgxl_ip=127.0.0.1`;
- remote/LAN AetherSDR: `server.bind_ip=<Windows LAN IP>` and `force_advertised_pgxl_ip=<same LAN IP>`.

## Phase 65 Verification

After changing the local band-follow profile to advertise `127.0.0.1`, the live
band-follow test produced:

- PGXL listener ready: `1779581479059`
- PGXL first accept: `1779581479245`
- Delay: about `0.186s`
- Amplifier create line: `ip=127.0.0.1 port=9008 ... state=STANDBY ...`
- PGXL direct session: connected, stable, 0 parse failures

This confirms the previous 30-40 second delay was an advertised-IP/listener-bind
mismatch in the local AetherSDR profile, not a PGXL server readiness problem.

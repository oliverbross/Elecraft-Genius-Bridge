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

Rust application code cannot observe raw TCP SYN packets before `accept()`. If SYN-level proof is needed, capture with Wireshark or Windows `pktmon`. The EGB-side proof is the combination of listener-ready timestamp, no early accepted PGXL session, and immediate local self-probe success.

Do not spend further time changing PGXL listener readiness or handshake timing unless future evidence shows an early accepted socket with a failed handshake.

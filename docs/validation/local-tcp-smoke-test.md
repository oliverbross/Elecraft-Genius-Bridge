# Local TCP Smoke Test

Date: 2026-05-17

Purpose: validate the mock bridge protocol surface without AetherSDR by using a simple TCP client against both emulator ports.

## Command

The test started `target\debug\egb.exe` with a temporary mock config:

```powershell
target\debug\egb.exe run --config $env:TEMP\egb-smoke\config.yaml
```

The temporary config enabled:

```yaml
logging:
  level: info
  protocol_trace: true
  protocol_transcript_dir: <temp>/egb-smoke/protocol
```

## PGXL Probe

Client connected to `127.0.0.1:9008`.

Observed:

```text
greeting=V0.1.0-egb-pgxl
info=R1|0|model=PowerGeniusXL serial_num=EGB-PGXL version=0.1.0-egb-pgxl
status=R2|0|state=STANDBY peakfwd=0.0000 swr=32.2557 temp=32.0 id=0.0 vac=230 meffa=OK fault=
```

Transcript excerpt:

```text
PGXL TX > V0.1.0-egb-pgxl
PGXL RX < C1|info
PGXL TX > R1|0|model=PowerGeniusXL serial_num=EGB-PGXL version=0.1.0-egb-pgxl
PGXL RX < C2|status
PGXL TX > R2|0|state=STANDBY peakfwd=0.0000 swr=32.2557 temp=32.0 id=0.0 vac=230 meffa=OK fault=
```

## TGXL Probe

Client connected to `127.0.0.1:9010`.

Observed:

```text
greeting=V0.1.0-egb-tgxl
info=R1|0|model=TunerGeniusXL serial_num=EGB-TGXL version=0.1.0-egb-tgxl one_by_three=1
status=R2|0|operate=0 bypass=0 tuning=0 relayC1=20 relayL=35 relayC2=20 antA=0 one_by_three=1 fwd=0.0000 swr=-32.2557
ant=R3|0|operate=0 bypass=0 tuning=0 relayC1=20 relayL=35 relayC2=20 antA=1 one_by_three=1 fwd=0.0000 swr=-32.2557
push=S0|state operate=0 bypass=0 tuning=0 relayC1=20 relayL=35 relayC2=20 antA=1 one_by_three=1 fwd=0.0000 swr=-32.2557
```

Transcript excerpt:

```text
TGXL TX > V0.1.0-egb-tgxl
TGXL RX < C1|info
TGXL TX > R1|0|model=TunerGeniusXL serial_num=EGB-TGXL version=0.1.0-egb-tgxl one_by_three=1
TGXL RX < C2|status
TGXL TX > R2|0|operate=0 bypass=0 tuning=0 relayC1=20 relayL=35 relayC2=20 antA=0 one_by_three=1 fwd=0.0000 swr=-32.2557
TGXL RX < C3|activate ant=2
TGXL TX > R3|0|operate=0 bypass=0 tuning=0 relayC1=20 relayL=35 relayC2=20 antA=1 one_by_three=1 fwd=0.0000 swr=-32.2557
TGXL TX > S0|state operate=0 bypass=0 tuning=0 relayC1=20 relayL=35 relayC2=20 antA=1 one_by_three=1 fwd=0.0000 swr=-32.2557
```

## Result

Pass.

- Both server-first `V` greetings were received.
- Phase 1 `info` and `status` commands returned expected responses.
- TGXL `activate ant=2` changed `antA` from `0` to `1` and emitted an unsolicited `S0|state` push.
- Per-client transcript files were created.


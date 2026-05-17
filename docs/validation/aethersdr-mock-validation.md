# AetherSDR Mock Validation

Use this checklist to validate the mock MVP against real AetherSDR on macOS.

## Bridge Setup

1. On the Windows bridge machine, start from the mock profile:

```powershell
Copy-Item config.mock.yaml config.yaml
```

2. Edit `config.yaml`:

```yaml
server:
  bind_ip: <windows-lan-ip>

kpa500:
  mock: true

kat500:
  mock: true

logging:
  level: debug
  protocol_trace: true
  protocol_transcript_dir: logs/protocol

tgxl:
  force_presence_test: true
```

3. Start EGB:

```powershell
cargo run -p egb -- run --config config.yaml
```

Or use the helper script:

```powershell
scripts\windows\run-mock.ps1
```

## AetherSDR PGXL Manual IP

1. Open AetherSDR on macOS.
2. Connect to the radio as normal.
3. Open Radio Setup.
4. Open the Peripherals tab.
5. Set Power Genius XL (PGXL):
   - IP address: Windows bridge LAN IP
   - Port: `9008`
6. Click Connect.
7. Verify AetherSDR shows PGXL connected.
8. Verify EGB logs contain:

```text
PGXL TX > V0.1.0-egb-pgxl
PGXL RX < C1|info
PGXL RX < C2|status
```

9. Verify status polling continues about every `200 ms`.
10. Verify displayed amp values:
   - State: STANDBY or equivalent idle display.
   - Forward power: near zero.
   - SWR: around `1.0` in no-RF mock mode.
   - Temperature: around `32 C`.
   - Mains voltage: `230 V`.

## AetherSDR TGXL Manual IP

1. In Radio Setup, Peripherals tab, set Tuner Genius XL (TGXL):
   - IP address: Windows bridge LAN IP
   - Port: `9010`
2. Click Connect.
3. Verify AetherSDR shows TGXL connected.
4. Verify EGB logs contain:

```text
TGXL TX > V0.1.0-egb-tgxl
TGXL RX < C1|info
TGXL RX < C2|status
```

5. Verify status polling continues about every `1000 ms`.
6. Verify displayed tuner values:
   - OPERATE/STANDBY/BYPASS state is coherent.
   - Forward power near zero.
   - SWR around `1.0` in no-RF mock mode.
   - Relay bars show C1/L/C2 values.
   - ANT 1/2/3 buttons appear if AetherSDR exposes them for mock TGXL.

## Command Validation

If controls are exposed in AetherSDR:

- Click TGXL Tune and confirm `TGXL RX < Cn|autotune`.
- Click TGXL antenna buttons and confirm `TGXL RX < Cn|activate ant=N`.
- Scroll/manual tune relay bars and confirm `TGXL RX < Cn|tune relay=<0|1|2> move=<+1|-1>`.
- Check whether PGXL operate/standby is exposed through direct TCP. If it is not, note that AetherSDR currently routes PGXL operate via the Flex amplifier API when a radio handle exists.

## Evidence To Capture

Save these artifacts under `docs/packet-captures/` or attach them to an issue:

- AetherSDR screenshot showing PGXL connected.
- AetherSDR screenshot showing TGXL connected.
- EGB log snippet with protocol trace enabled.
- Files from `logs/protocol/`.
- Any AetherSDR disconnect or parse error messages.

## Pass Criteria

- Both devices connect by manual IP.
- No reconnect loop.
- Polling is visible in logs.
- Displayed values are coherent.
- Exposed TGXL controls produce expected commands.
- Transcript files are created for both client sessions.

## Exact Next Manual Test A: macOS/AetherSDR Mock

1. On Windows, edit `config.mock.yaml` and set `server.bind_ip` to the Windows LAN IP.
2. Run:

```powershell
scripts\windows\run-mock.ps1
```

3. On macOS, open AetherSDR and connect to the radio as usual.
4. Configure PGXL manual IP to the Windows LAN IP and port `9008`.
5. Configure TGXL manual IP to the Windows LAN IP and port `9010`.
6. Save screenshots showing PGXL and TGXL connected.
7. Save transcript files from `logs/protocol/`.
8. Copy `docs/validation/aethersdr-session-report-template.md` into a session-specific report and fill in mismatches.
9. Fill out `docs/validation/aethersdr-applet-visibility-test.md`.
10. If TUN remains hidden, follow `docs/validation/aethersdr-binary-source-match.md` and `docs/validation/aethersdr-layout-reset.md`.

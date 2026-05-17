# Elecraft Hardware Validation

Use this only after mock AetherSDR validation passes.

## Safety First

- Use a dummy load for RF tests.
- Start at the lowest practical RF drive.
- Do not perform unattended transmit or tune tests.
- Confirm the KPA500 and KAT500 are correctly cabled and grounded.
- For first serial tests, use no RF.

## Windows COM Port Discovery

```powershell
egb list-serial
```

Expected initial ports:

- KPA500: `COM21`
- KAT500: `COM8`

Adjust `config.yaml` if Windows reports different ports.

## Enable Debug Logs And Transcripts

```yaml
logging:
  level: debug
  protocol_trace: true
  protocol_transcript_dir: logs/protocol
  serial_transcript_dir: logs/serial

kpa500:
  mock: false
  dry_run: true

kat500:
  mock: false
  dry_run: true
```

## KPA500 Test Steps

1. Confirm KPA500 is powered on and connected by USB/serial.
2. Probe firmware first:

```powershell
.\target-msvc\debug\egb.exe serial-probe --port COM21 --baud 38400 --send "^RVM;" --timeout-ms 1000
```

3. Run:

```powershell
egb test-kpa --config config.yaml
```

4. Confirm the safety summary shows only caret-prefixed read-only KPA500 commands. Do not use `--allow-control` or `--allow-rf-risk` during the first read-only test.
5. Start bridge:

```powershell
egb run --config config.yaml
```

6. Confirm serial open succeeds.
7. Confirm no fault is reported.
8. Do not transmit yet.
9. If operate/standby commands are tested later, use `config.hardware-control-local-only.yaml`, watch the physical amp, and be ready to stop EGB.

## KAT500 Test Steps

1. Confirm KAT500 is powered on and connected by USB/serial.
2. Probe wake/baud first:

```powershell
.\target-msvc\debug\egb.exe baud-scan --port COM8
```

3. Run:

```powershell
egb test-kat --config config.yaml
```

4. Confirm the safety summary shows wake/baud discovery and documented read-only KAT500 GETs. Do not use `--allow-control` or `--allow-rf-risk` during the first read-only test.
5. Start bridge.
6. Confirm serial open succeeds.
7. Do not run a tune cycle with RF until dummy load and low-power drive are ready.

## Low-Power RF Test

1. Use dummy load.
2. Set radio drive to the lowest safe value.
3. Confirm KPA500 remains stable in standby.
4. Confirm KAT500 reports sane status.
5. Try TGXL mock-equivalent tune flow only after serial commands are verified.

## Serial Transcript Capture

Set:

```yaml
logging:
  serial_transcript_dir: logs/serial
```

EGB writes timestamped files such as:

```text
logs/serial/kpa500-<timestamp>-COM21.log
logs/serial/kat500-<timestamp>-COM8.log
```

Keep these with the hardware validation notes. Also record exact firmware versions and physical device behavior for each command.

## Rollback

1. Stop EGB with Ctrl+C.
2. Set both devices back to mock mode:

```yaml
kpa500:
  mock: true

kat500:
  mock: true
```

3. Restart AetherSDR if it keeps stale connection state.
4. Power-cycle Elecraft hardware only if the device manual recommends it for the observed fault.

## Exact Next Manual Test B: Windows Hardware Read-Only

1. Connect KPA500 on `COM21` and KAT500 on `COM8`.
2. Run:

```powershell
cargo run -p egb -- list-serial
cargo run -p egb -- check-config --config config.hardware-readonly.yaml
cargo run -p egb -- test-kpa --config config.hardware-readonly.yaml
cargo run -p egb -- test-kat --config config.hardware-readonly.yaml
```

3. Verify the CLI summaries show only read-only `poll_status` will be sent.
4. Save files from `logs/serial/`.
5. Verify no operate, tune, antenna, bypass, relay move, or clear-fault command appears in the serial transcripts.

## Exact Next Manual Test C: Local-Only Hardware Control

Use this only after Test B passes.

1. Keep `config.hardware-control-local-only.yaml` bound to `127.0.0.1` or replace it with a private LAN IP only. Do not expose it to WAN.
2. Use a dummy load or no RF, depending on the specific device action.
3. Run read-only tests first:

```powershell
cargo run -p egb -- test-kpa --config config.hardware-control-local-only.yaml
cargo run -p egb -- test-kat --config config.hardware-control-local-only.yaml
```

4. Test standby/operate only if physically safe:

```powershell
cargo run -p egb -- test-kpa --config config.hardware-control-local-only.yaml --allow-control
```

5. Test KAT500 bypass only if physically safe:

```powershell
cargo run -p egb -- test-kat --config config.hardware-control-local-only.yaml --allow-control
```

6. Test KAT500 autotune only with dummy load and a safe low-power plan:

```powershell
cargo run -p egb -- test-kat --config config.hardware-control-local-only.yaml --allow-rf-risk
```

7. Save serial transcripts and stop immediately on unexpected physical device behaviour.

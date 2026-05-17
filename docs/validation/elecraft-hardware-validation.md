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

kpa500:
  mock: false

kat500:
  mock: false
```

## KPA500 Test Steps

1. Confirm KPA500 is powered on and connected by USB/serial.
2. Run:

```powershell
egb test-kpa --config config.yaml
```

3. Start bridge:

```powershell
egb run --config config.yaml
```

4. Confirm serial open succeeds.
5. Confirm no fault is reported.
6. Do not transmit yet.
7. If operate/standby commands are tested, watch the physical amp and be ready to stop EGB.

## KAT500 Test Steps

1. Confirm KAT500 is powered on and connected by USB/serial.
2. Run:

```powershell
egb test-kat --config config.yaml
```

3. Start bridge.
4. Confirm serial open succeeds.
5. Do not run a tune cycle with RF until dummy load and low-power drive are ready.

## Low-Power RF Test

1. Use dummy load.
2. Set radio drive to the lowest safe value.
3. Confirm KPA500 remains stable in standby.
4. Confirm KAT500 reports sane status.
5. Try TGXL mock-equivalent tune flow only after serial commands are verified.

## Serial Transcript Capture

Serial transcript capture is not implemented yet. For now:

- Keep `logging.level: debug`.
- Save console output.
- Record exact firmware versions.
- Record physical device behavior for each command.

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


# AetherSDR PGXL Open Trigger

Source inspected: `research/AetherSDR`.

## Trigger Path

`src/models/RadioModel.cpp` parses Flex radio `amplifier <handle> ...` status lines.

For a power amplifier:

- `model` must be non-empty and not `TunerGeniusXL`.
- `m_ampHandle` is set to the amplifier handle.
- On first presence, `m_hasAmplifier` becomes true.
- `m_ampIp` is set from the `ip` field.
- `amplifierChanged(true)` is emitted.

`src/gui/MainWindow.cpp` connects that signal:

```text
RadioModel::amplifierChanged(true)
  if ampIp is not empty and PGXL direct is not connected:
    PgxlConnection::connectToPgxl(ampIp)
```

Therefore AetherSDR should attempt TCP 9008 only after it receives a radio-side amplifier status line with:

- a non-`TunerGeniusXL` `model`, normally `PowerGeniusXL`,
- an `ip` field,
- a handle,
- and no removal marker.

The inspected source does not require `connected=1`, `configured=1`, `enabled=1`, `direct=1`, or `lan=1` for the direct TCP open. Those fields are stripped by Flex in recent evidence and are not read by the inspected parser.

## Timing Implication

If EGB has already sent/triggered amplifier registration and PGXL self-probe succeeds, but no PGXL TCP accept occurs, the delay is AetherSDR-side signal timing, parser eligibility, manual-connect retry timing, or installed-binary behaviour.

Use:

```powershell
.\target\release\egb.exe aethersdr-open-trigger-test --config .\config.aethersdr-last-known-good-real-controls.yaml --variant state_only --duration-seconds 120
```

The test varies only the Flex amplifier advertisement/create fields and reports whether the PGXL TCP accept happens within the run.

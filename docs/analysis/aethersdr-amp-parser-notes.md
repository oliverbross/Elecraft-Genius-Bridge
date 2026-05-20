# AetherSDR Amp Parser Notes

Source inspected:

- `research/AetherSDR/src/models/RadioModel.cpp`
- `research/AetherSDR/src/gui/MainWindow.cpp`

Findings:

- AetherSDR parses Flex status/event lines matching `amplifier <handle> ...`.
- `model=TunerGeniusXL` is treated as TGXL/TUN.
- Any non-empty `model` that is not `TunerGeniusXL` is treated as a power amplifier.
- On first power amplifier status, AetherSDR stores the `ip` field as `m_ampIp`, stores the model, and emits `amplifierChanged(true)`.
- `MainWindow` reacts to `amplifierChanged(true)` by calling `connectToPgxl(m_radioModel.ampIp())` if the IP is non-empty and PGXL direct is not already connected.
- The auto-connect path uses the IP; the manual Peripherals UI defaults PGXL to port `9008`.
- Operate state is derived from `state=IDLE`, `state=OPERATE`, or `state=TRANSMIT*`. `STANDBY` is valid but not an operate state.

Working hypothesis:

If AetherSDR receives a Flex amplifier status line containing `model=PowerGeniusXL` and a non-empty `ip`, it should attempt PGXL TCP 9008. If that does not happen, the next evidence to collect is whether Flex stripped fields before AetherSDR saw the line, whether the installed binary differs from this source, or whether a local peripheral/UI setting suppresses direct auto-connect.

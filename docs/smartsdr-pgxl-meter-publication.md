# SmartSDR PGXL Meter Publication

EGB creates the documented PGXL AMP meters:

- `FWD`
- `RL`
- `DRV`
- `ID`
- `TEMP`

SmartSDR currently sees the virtual amplifier object but does not show useful live PGXL data. The missing path is live meter value publication back into Flex/SmartSDR.

Current result:

- Meter objects are created and handles are tracked.
- KPA500 values are available inside EGB.
- No verified Flex TCP command has been captured that allows an external amplifier client to publish arbitrary meter values for those handles.
- `/status.flex_diagnostics.meter_publish_supported` reports `false`.

Until a verified command/capture exists, EGB keeps live values authoritative on the direct PGXL TCP status path and documents SmartSDR PGXL meter display as partial.

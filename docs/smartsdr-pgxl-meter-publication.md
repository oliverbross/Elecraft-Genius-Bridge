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

Phase 68 rechecked the public Flex API material and a FlexRadio community
thread on SmartSDR amplifier meters. The useful evidence is:

- SmartSDR TCP status defines meter objects and clients receive meter values as
  streaming data, not as ordinary TCP status fields.
- A FlexRadio administrator stated that LAN-side external meter UDP packets
  should be sent to the radio IP on port `4991`, and referenced FlexLib
  `VitaMeterPacket` formatting for packet details.
- The same thread shows AMP meter definitions with `src=AMP` and meter handles,
  but no simple documented TCP `meter set` command for publishing values.

References:

- https://github-wiki-see.page/m/flexradio/smartsdr-api-docs/wiki/SmartSDR-TCPIP-API
- https://community.flexradio.com/discussion/8032341/smartsdr-amplifier-meters

Until a verified command/capture exists, EGB keeps live values authoritative on the direct PGXL TCP status path and documents SmartSDR PGXL meter display as partial.

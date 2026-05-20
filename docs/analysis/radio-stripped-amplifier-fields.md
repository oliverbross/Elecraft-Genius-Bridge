# Radio-Stripped Amplifier Fields

EGB now records both sides of the amplifier advertisement path:

- `last_emitted_amplifier_advertisement_line`: what EGB attempted to register or reannounce.
- `last_amplifier_status_line`: what EGB observed back from the Flex API subscription/status stream.

The evidence bundle file `radio-stripped-amplifier-fields.md` compares key/value field names between the two lines.

If fields such as `port`, `connected`, `configured`, `enabled`, `direct`, or `lan` are present in the emitted line but absent in the observed line, then AetherSDR cannot rely on those fields because the radio/API path is stripping them.

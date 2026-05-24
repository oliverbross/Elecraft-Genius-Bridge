# Real PGXL vs EGB Behaviour

This review compares the expected Flex ecosystem role of a real Power Genius XL
with the current EGB virtual PGXL.

## Matching Behaviour

EGB now matches these important behaviours:

- Connects to Flex TCP API port `4992`.
- Waits for the Flex client handle.
- Creates a PowerGeniusXL amplifier object with stable serial and antenna map.
- Creates the documented AMP meter objects.
- Creates an AMP interlock unless disabled for a test.
- Enables keepalive and pings periodically.
- Subscribes to amplifier, slice, and TX state.
- Maintains one amplifier object per Flex session.
- Does not recreate the amplifier during KPA telemetry changes.
- Serves direct PGXL TCP on port `9008`.
- Reports direct PGXL status from live KPA500 telemetry.

## Compatibility Behaviour

For AetherSDR, the working profile includes a create-time `state=<live-state>`
field. Flex echoes only supported fields back to clients. This is retained as an
AetherSDR compatibility trigger, not as the strict official profile.

## Known Differences

Real PGXL behaviour that EGB does not fully reproduce yet:

- Live AMP meter values are not published as VITA-49 meter packets to the radio.
- SmartSDR PGXL meter display is therefore incomplete.
- Real PGXL and TGXL may have a paired topology with richer interlock and tuner
  association than the public PGXL amplifier-to-radio document exposes.
- SmartSDR TGXL visibility is not reproduced because no verified external TGXL
  Flex registration command sequence is implemented.
- EGB maps real RF operate to KPA500 only when explicit RF-risk gates allow it.

## Control Path

Real Flex clients command the amplifier through the radio-side amplifier API.
EGB therefore watches Flex amplifier status for both:

- `operate=0|1`
- external-client `state=STANDBY|OPERATE`

Direct PGXL TCP remains telemetry/status in the inspected AetherSDR source. If a
client never emits a Flex-side control indication, EGB cannot infer an operator
intent safely.


# SmartSDR Tuner Success

Status: Phase 19 validation note.

## Confirmed

SmartSDR sees the injected tuner. This proves the radio-side accessory path is viable for EGB and that a direct socket bridge alone is not the only workable integration mechanism.

## Current Gap

SmartSDR does not yet see the injected amplifier with the earlier amplifier-create-only path.

## Hypothesis

A real PGXL registers additional radio-side objects beyond `amplifier create`:

- AMP meters.
- AMP interlock.
- Keepalive.
- Periodic ping.
- Matching serial and antenna map metadata.

Phase 19 implements those registration commands. The next validation should check whether SmartSDR now shows the amplifier and whether the radio returns handles for the amplifier, meters, or interlock.

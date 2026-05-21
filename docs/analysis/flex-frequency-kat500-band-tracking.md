# Flex Frequency Tracking and KAT500 Tune Context

## Implemented Source of Truth

EGB now tracks radio context from the Flex TCP API by subscribing to:

- `sub slice all`
- `sub tx all`

The bridge parses Flex status lines such as:

```text
S...|slice 0 RF_frequency=14.200000 tx=1 mode=USB txant=ANT1 rxant=ANT1
S...|transmit tx_ant=ANT1 rx_ant=ANT1
```

The active TX slice is preferred. If no TX slice has been observed yet, the first slice with a valid frequency becomes a fallback radio context until a TX slice arrives.

The tracked context is exposed at:

```text
GET /status
radio_context.frequency_hz
radio_context.band
radio_context.mode
radio_context.tx_antenna
radio_context.rx_antenna
```

Evidence is written to:

```text
radio-context.json
```

inside the active evidence run directory when evidence capture is enabled.

## Band Mapping

Band is derived locally from frequency using standard amateur band ranges:

- 160m: 1.800-2.000 MHz
- 80m: 3.500-4.000 MHz
- 60m: 5.3305-5.4065 MHz
- 40m: 7.000-7.300 MHz
- 30m: 10.100-10.150 MHz
- 20m: 14.000-14.350 MHz
- 17m: 18.068-18.168 MHz
- 15m: 21.000-21.450 MHz
- 12m: 24.890-24.990 MHz
- 10m: 28.000-29.700 MHz
- 6m: 50.000-54.000 MHz

## KAT500 Band/Frequency Limitation

The currently hardware-verified KAT500 command set is:

```text
RV;
SN;
AN;
BYP;
MD;
TP;
FLT;
VSWR;
VFWD;
T;
```

No verified KAT500 command is currently available in this project for directly setting a frequency or band before `T;`.

Therefore EGB does **not** send an unverified KAT500 band-set command. On each AetherSDR TGXL `autotune` command, EGB records the latest Flex TX frequency/band and then sends only the verified KAT500 tune command when controls are enabled:

```text
T;
```

Tune evidence is written to:

```text
tune-band-decision.md
kat500-tune-sequence.log
```

## AetherSDR TGXL Status Fields

The inspected AetherSDR TGXL model currently consumes direct TGXL fields including:

- `operate`
- `bypass`
- `tuning`
- `relayC1`
- `relayL`
- `relayC2`
- `antA`
- `one_by_three`
- `fwd`
- `swr`
- `ip`
- identity fields such as `serial_num` and `model`

No source evidence was found that AetherSDR consumes band or frequency fields from direct TGXL TCP status. EGB therefore keeps TGXL direct status limited to fields already observed in the AetherSDR model and exposes frequency context through `/status` and evidence instead.

## PGXL Widget Stability Notes

PGXL direct TCP stability is not the only condition for AetherSDR AMP widget stability. The AetherSDR source also tracks radio-side amplifier presence and state from the Flex API. EGB now exposes diagnostic fields for this separation:

```text
pgxl_lifecycle
flex_diagnostics.amp_widget_visibility_risk
flex_diagnostics.amplifier_reannounce_count
flex_diagnostics.amplifier_handle_change_count
```

If PGXL TCP remains connected while the AMP widget appears/disappears, the likely cause is Flex amplifier status churn, missing handle state, or advertised-state mismatch rather than PGXL socket framing.

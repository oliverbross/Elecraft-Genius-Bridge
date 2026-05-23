# KPA500 Band/Frequency Follow

The official Elecraft KPA500 Programmer's Reference lists `^BN` as **Band Selection** and documents `^BNnn;` where `nn` selects the band (`00=160m`, `01=80m`, `03=40m`, `05=20m`, `07=15m`, etc.). The same reference does not provide a direct frequency-in-Hz set command for the amplifier.

Source: [Elecraft KPA500 Programmer's Reference](https://ftp.elecraft.com/KPA/Manuals%20Downloads/KPA500%20Programmers%20Ref.pdf).

## Current EGB Behavior

- EGB continuously tracks Flex TX slice frequency and band.
- PGXL/TGXL advertised state uses the current Flex radio context.
- KAT500 can receive `F <kHz>;` frequency context and now follows Flex frequency continuously when enabled.
- KPA500 automatic band following is available only behind `kpa500.follow_flex_band: true`.

## Experimental KPA Band Follow

When enabled, EGB maps the current Flex band to `^BNnn;` and sends it only when:

- `kpa500.follow_flex_band: true`
- KPA dry-run is off
- KPA control is allowed
- the amplifier is not reporting transmit state
- the requested band changed from the last sent band

Evidence is written to `kpa500-band-follow.log`, and `/status.radio_context` exposes the last KPA band-follow wire/result plus sent/skipped counters.

This remains experimental because `^BNnn;` is a band command, not a frequency command, and changing amplifier band from the bridge has RF-operational implications.

## Limitation

KPA500 cannot be directly frequency-followed by a verified serial frequency command. EGB can update PGXL/Flex-side frequency and band state from Flex, and KAT500 can be given `F <kHz>;`. KPA500 can only be band-followed experimentally with `^BNnn;`.

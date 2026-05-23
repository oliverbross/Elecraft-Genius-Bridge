# KPA500 Band/Frequency Follow

The official Elecraft KPA500 Programmer's Reference lists `^BN` as **Band Selection** and documents `^BNnn;` where `nn` selects the band (`00=160m`, `01=80m`, `03=40m`, `05=20m`, `07=15m`, etc.). The same reference does not provide a direct frequency-in-Hz set command for the amplifier.

Source: [Elecraft KPA500 Programmer's Reference](https://ftp.elecraft.com/KPA/Manuals%20Downloads/KPA500%20Programmers%20Ref.pdf).

## Current EGB Behavior

- EGB continuously tracks Flex TX slice frequency and band.
- PGXL/TGXL advertised state uses the current Flex radio context.
- KAT500 can receive `F <kHz>;` frequency context and now follows Flex frequency continuously when enabled.
- KPA500 automatic band following is not enabled in this phase.

## Reason

`^BNnn;` is a verified band-selection command, but it is a band command, not a frequency command, and changing amplifier band from the bridge has RF-operational implications. The current requirement is to preserve stable KPA500 polling/control while fixing AetherSDR/Flex-side synchronization. EGB therefore does not automatically send `^BNnn;` until a separate controlled KPA band-follow validation is performed.

## Limitation

KPA500 cannot be directly frequency-followed by a verified serial frequency command. EGB can update PGXL/Flex-side frequency and band state from Flex, and KAT500 can be given `F <kHz>;`, but KPA500 band forcing would require a future guarded `^BNnn;` implementation.

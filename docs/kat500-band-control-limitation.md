# KAT500 Band and Frequency Control

The KAT500 serial reference includes documented band/frequency context commands:

- `BN;` reads the current band.
- `BNbb;` switches the ATU to band number `00..10`, selecting the last-used or preferred antenna and recalling recent relay settings for that band.
- `F;` reads the current ATU frequency.
- `F nnnnn;` sets the frequency in kHz used for ATU setting selection.

EGB now uses the frequency-context path before an enabled TGXL autotune:

```text
F <current Flex TX frequency in kHz>;
T;
TP;
VSWR;
```

This lets the KAT500 recall the best matching memory/bin for the current Flex TX frequency before a tune request. EGB does not send raw relay `C`/`L` commands and does not erase or save KAT500 memories automatically.

## Safety Boundary

`F nnnnn;` can cause the KAT500 to change bands and recall stored relay settings. EGB sends it only as part of an already-authorized tune sequence. If KAT tune is blocked by dry-run or RF-risk policy, the frequency-context command is skipped.

## Remaining Limitation

This is not a guarantee that the KAT500 will behave identically to a TGXL. The KAT500 owns its own memory and relay selection model. EGB can provide the current frequency context, but it cannot force TGXL-style band memory semantics without sending lower-level relay commands, which remain intentionally unsupported until separately verified.

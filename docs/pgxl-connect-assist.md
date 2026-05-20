# PGXL Connect Assist

`flex_injection.pgxl_connect_assist` is an AetherSDR compatibility mode.

When enabled, EGB sends one Flex API command after the injected amplifier handle is observed:

```text
amplifier set <handle> operate=1
```

This is a UI/connect trigger only. It does not send `^OS1;` to the KPA500 and does not change the real amplifier RF state.

The real state remains authoritative:

- KPA500 `^OS0;` -> PGXL direct status reports `state=STANDBY`.
- KPA500 `^OS1;` -> PGXL direct status reports `state=OPERATE`.

Evidence files:

- `pgxl-connect-assist.md`
- `real-vs-ui-amp-state.md`
- `aethersdr-operational-test.md`
- `flex-amplifier-operate-sequence.log`

Recommended test:

```powershell
.\target-msvc\debug\egb.exe aethersdr-operational-test --config .\config.aethersdr-known-good.yaml --duration-seconds 90
```

During the run, connect AetherSDR to the radio and watch for:

- PGXL direct session start.
- TGXL direct session start.
- AMP/TUN applet stability.
- Any button presses and safety blocks.

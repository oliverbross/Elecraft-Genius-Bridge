# Flex OPERATE Reflection Status

PGXL direct status now reflects external KPA500 operate/standby changes quickly.
Recent production evidence showed direct PGXL status matching external KPA state
within roughly `46-158 ms`.

Flex-side amplifier status behaves differently.

## Evidence

For `STANDBY -> OPERATE` transitions, EGB emitted Flex amplifier reannounce
lines containing:

```text
state=OPERATE
```

The radio then echoed amplifier status lines containing:

```text
state=STANDBY
```

For `OPERATE -> STANDBY`, the echoed Flex status matched `STANDBY`.

This means the EGB shared state and generated amplifier line are correct for
OPERATE, but the Flex radio owns or rewrites the amplifier object's radio-side
operate state back to STANDBY.

## Runtime Evidence Going Forward

Every KPA state transition now records the exact emitted and echoed lines in:

```text
kpa-state-transition-latency.jsonl
flex-state-reflection-evidence.md
```

Important fields:

- `first_flex_reannounce_line`
- `first_flex_status_line`
- `first_flex_matching_state_ms`
- `flex_state_reflection_result`

If `first_flex_reannounce_line` contains `state=OPERATE` but
`first_flex_status_line` contains `state=STANDBY`, the result is a Flex-side
rewrite/ownership issue, not stale EGB KPA state.

## Current Conclusion

No EGB fix is applied to force Flex radio-side OPERATE. The production baseline
keeps real KPA state authoritative for PGXL direct status and avoids Flex
operate hacks. Earlier `amplifier set <handle> operate=1` connect-assist
experiments were rejected by Flex and are not production machinery.


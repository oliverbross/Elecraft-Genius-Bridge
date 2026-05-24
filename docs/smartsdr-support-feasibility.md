# SmartSDR Support Feasibility

Phase 65 separates what is currently supported in SmartSDR from what is only
supported in AetherSDR.

## PGXL / Amplifier

Status: partial.

EGB can register a Flex amplifier object that SmartSDR can see as a PGXL-style
amplifier. The registration path creates the amplifier object, meter handles,
and optional interlock.

Remaining blocker:

- EGB creates meter objects but does not publish live meter values through a
  verified Flex API command. Latest runs show `meter_publish_count=0`.

Conclusion:

- SmartSDR PGXL object visibility is feasible.
- SmartSDR PGXL live telemetry is incomplete until a documented or captured
  client-side meter publication mechanism is implemented.

## TGXL / Tuner

Status: unsupported at this time.

AetherSDR supports direct TGXL TCP on port 9010 and uses that path for the Tuner
applet and Tune commands. SmartSDR does not use EGB's direct TGXL TCP path.

No verified Flex command was found in the reviewed sources/docs that lets an
external client register a Tuner Genius XL object equivalent to the amplifier
create flow.

Conclusion:

- SmartSDR TGXL should remain documented as unsupported unless a real Flex-side
  TGXL/tuner registration API is found or captured from a real TGXL.
- SmartSDR frequency changes still feed EGB through Flex slice tracking, so KAT
  frequency follow can work even without a visible SmartSDR tuner widget.

## Interlock

Status: testable, not a complete SmartSDR compatibility solution.

EGB can create or intentionally disable the AMP interlock for test runs. If the
Flex interlock reports `tx_allowed=1` with an empty `amplifier=` field, EGB treats
that as warning-level evidence. If `tx_allowed=0`, EGB treats it as an
interlock-blocked condition.

Disabling the interlock can prove whether TX blocking is caused by the injected
interlock, but it is a test mode only. It does not make SmartSDR TGXL supported
and does not send KPA operate.

## Recommended Support Statement

- AetherSDR: supported primary client for PGXL/TGXL direct bridge operation.
- SmartSDR PGXL: experimental/partial; object visibility works, live meters are
  not complete.
- SmartSDR TGXL: unsupported until a verified Flex tuner-registration mechanism
  is available.

# KPA500 Unsolicited Response Handling

Live Phase 67 evidence showed repeated `^SP1;` lines appearing while EGB was
waiting for expected KPA500 replies such as `^OS;` and `^VI;`.

## Behaviour

EGB now treats these as unsolicited KPA status, not command failures:

- `^SP...;`
- `^ST...;`
- already-supported telemetry such as `^OS`, `^WS`, `^TM`, `^VI`, `^FL`, and
  `^BN`

When such a line arrives while another command is waiting:

1. The line is recorded as unsolicited.
2. It is added to the command outcome's unsolicited list.
3. The driver continues reading until the expected prefix arrives or the command
   times out.
4. The line is logged as `unsolicited_kpa_status` when it is `^SP...;`.

Evidence files:

- `kpa500-serial.log`
- `kpa500-unsolicited.log`

## Meaning

`^SP1;` is not currently mapped to authoritative PGXL/KPA state because the
verified operational state remains `^OS0;` / `^OS1;`. If a future KPA500
Programmer Reference transcript proves an exact semantic for `^SP`, it can be
promoted from logged unsolicited status to parsed telemetry.

## State Reflection

The state-reflection path remains tied to `^OS`:

- Any unsolicited `^OS...;` line updates shared KPA state immediately.
- If unrelated `^SP...;` lines arrive first, EGB keeps reading until `^OS` or
  timeout.
- KPA state changes request the bounded Flex reannounce burst and update PGXL
  direct status from the shared state.

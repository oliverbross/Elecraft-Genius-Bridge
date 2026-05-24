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

The Elecraft KPA500 Programmer's Reference defines `^SP` as fault speaker
on/off. `^SP1;` means the fault speaker is enabled; it is configuration
telemetry, not operate/standby state and not a command failure.

EGB parses unsolicited `^SP0;` / `^SP1;` into `amp.fault_speaker_on`, but the
verified operational state remains `^OS0;` / `^OS1;`.

## State Reflection

The state-reflection path remains tied to `^OS`:

- Any unsolicited `^OS...;` line updates shared KPA state immediately.
- If unrelated `^SP...;` lines arrive first, EGB keeps reading until `^OS` or
  timeout.
- KPA state changes request the bounded Flex reannounce burst and update PGXL
  direct status from the shared state.

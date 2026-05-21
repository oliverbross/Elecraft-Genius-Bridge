# Lifecycle Timing Analysis

Phase 46 evidence bundles now include lifecycle timing and transition data for the major objects:

- `/status.lifecycle.flex_session`
- `/status.lifecycle.amplifier`
- `/status.lifecycle.pgxl`
- `/status.lifecycle.tgxl`
- `/status.lifecycle.aether_client`
- `/status.lifecycle.tune`

## Evidence Files

- `lifecycle-events.jsonl`: duplicate Flex commands, amplifier handle observations, and lifecycle-significant events.
- `amplifier-removal-timeline.md`: timeline captured when Flex reports `amplifier <handle> removed`.
- `tgxl_state_transition.log`: event-driven Flex slice/TX updates that change TGXL advertised band/frequency context.
- `kat500-tune-sequence.log`: Tune request, duplicate suppression, command execution, and follow-up polling.
- `ecosystem-soak-report.md`: summary produced by `egb ecosystem-soak-test`.

## What To Look For

Healthy PGXL lifecycle:

1. Flex connects and receives `H<handle>`.
2. Registration sequence is sent once.
3. Flex accepts amplifier create.
4. A stable amplifier handle is observed.
5. AetherSDR opens TCP 9008.
6. PGXL remains `PGXL_STABLE` or lifecycle `active` without removal events.

Unhealthy PGXL lifecycle:

- `duplicate_amplifier_create_count > 0`
- `amplifier_removed_count > 0`
- `amplifier_handle_change_count` increases without a Flex reconnect
- `amplifier-removal-timeline.md` shows removal shortly after connect-assist, interlock, or a rejected command

Healthy TGXL/Tune lifecycle:

1. Flex slice/TX status updates `/status.radio_context`.
2. TGXL status uses the current `freqA`, `bandA`, and `modeA`.
3. AetherSDR Tune creates one `autotune` event.
4. KAT500 receives one `T;` unless duplicate suppression is inside the cooldown window.
5. Tune lifecycle returns to `idle`.

Unhealthy TGXL/Tune lifecycle:

- Tune lifecycle remains stuck at `cooldown`, `tuning`, or `tune-failed`.
- `duplicate_autotune_suppressed_count` rises while user presses Tune at normal intervals.
- `tgxl_state_transition.log` stops updating after Flex band changes.


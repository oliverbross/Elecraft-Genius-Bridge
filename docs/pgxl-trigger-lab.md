# PGXL Trigger Lab

The trigger lab runs EGB with one Flex amplifier status profile and records whether AetherSDR opens direct PGXL TCP `9008`.

```powershell
.\target-msvc\debug\egb.exe pgxl-trigger-lab --config .\config.smartsdr-stability.yaml --profile pgxl_paired --duration-minutes 5
```

Profiles:

- `minimal`: documented `amplifier create` fields only.
- `pgxl_paired`: current normal profile; documented create fields plus evidence logging of a paired PGXL status line.
- `pgxl_verbose`: experimental; adds `connected=1 configured=1 enabled=1` to the create command.
- `old_good_pgxl`: last-known-good regression profile with direct-connect readiness fields.
- `aethersdr_force_direct`: recommended AetherSDR test profile; adds verbose fields plus `direct=1 lan=1`.

Experimental fields are isolated behind the profile because they are not confirmed as accepted by real Flex firmware.

Evidence files:

- `pgxl-trigger-analysis.md`
- `amplifier-status-lines.log`
- `amplifier-reannounce.log`
- `pgxl-protocol.log`
- `client-sessions.jsonl`
- `summary.md`

Interpretation:

- `pgxl_session_started_count > 0`: AetherSDR opened TCP `9008`.
- `pgxl_session_started_count = 0` with amplifier present: AetherSDR did not attempt direct PGXL TCP.
- `pgxl_manual_connect_no_socket_attempt`: EGB saw radio-side amplifier presence but no direct PGXL socket attempt within the watcher window.

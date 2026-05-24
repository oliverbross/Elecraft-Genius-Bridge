# Configuration

See `config.example.yaml` for the authoritative example.

## Server

```yaml
server:
  bind_ip: 127.0.0.1
```

Default is loopback for safety. Use a LAN IP for AetherSDR on another machine. Avoid `0.0.0.0` unless you understand the security implications.

## Emulators

```yaml
pgxl:
  enabled: true
  port: 9008
  aethersdr_compat: false
  status_profile: status_current
  strict_emulation: false
  startup_delay_ms: 0

tgxl:
  enabled: true
  port: 9010
  aethersdr_compat: false
  control_profile: readonly
  strict_emulation: false
  startup_delay_ms: 0
  force_presence_test: false
```

Set `aethersdr_compat: true` while diagnosing AetherSDR direct PGXL/TGXL behaviour. Compatibility mode emits only source-observed fields, removes unverified `capabilities`, `firmware`, `connection_state`, and `fault` fields from protocol bodies, and reports `swr` as return loss dB because AetherSDR converts it back to an SWR ratio.

Set `pgxl.status_profile` only for direct PGXL button-gating experiments. `status_current` preserves the current proven status body. `status_control_fields` and `status_operate_capable` append `operate_capable=1 standby_capable=1`. `status_rich_metered` and `status_real_pgxl_like` append progressively richer capability/telemetry fields. `status_realistic_operate` and `status_realistic_standby` force only the direct PGXL status state for eligibility testing. These profiles do not change Flex amplifier create fields or real KPA500 control gates.

Set `strict_emulation: true` in mock mode to simulate a more realistic device startup sequence. The emulator sends the required `V` greeting immediately, but shared mock state reports transitional readiness until `startup_delay_ms` expires.

Set `tgxl.force_presence_test: true` only for AetherSDR TUN applet activation testing. It makes the direct TGXL emulator publish the richest safe direct state currently understood, without changing KAT500 serial behaviour.

Set `tgxl.control_profile` to `control_ready` only for AetherSDR/SmartSDR control-button experiments. It reports a control-ready TGXL direct state while the serial driver still enforces `dry_run` and RF-risk gates. Valid values are `readonly`, `control_ready`, and `verbose_control`. The older names `tgxl_control_ready` and `tgxl_verbose_control` are accepted only for backward compatibility.

## Elecraft Devices

```yaml
kpa500:
  enabled: true
  com_port: COM21
  baud: 38400
  polling_interval_ms: 1000
  mock: true
  dry_run: true
  allow_control: false
  allow_rf_risk: false
  follow_flex_band: false

kat500:
  enabled: true
  com_port: COM8
  baud: 38400
  polling_interval_ms: 1000
  mock: true
  dry_run: true
  allow_control: false
  allow_rf_risk: false
  follow_flex_frequency: false
```

Set `mock: false` only when real hardware is connected and command mappings have been checked for your firmware.

Set `dry_run: true` for first hardware tests. Dry-run opens the configured COM port and permits read-only status queries, but blocks control-changing commands such as operate, tune, antenna change, bypass, relay move, and clear fault.

Set `kpa500.allow_rf_risk: true` only for local controlled KPA500 operate testing after dummy-load / no-RF checks. It is required before EGB will translate a Flex/PGXL operate request to `^OS1;`. Standby still uses `^OS0;` and remains state-change-safe, but it is blocked when `dry_run: true`.

Set `kpa500.follow_flex_band: true` only for experimental local testing. It sends KPA500 `^BNnn;` on Flex band changes when dry-run is off and KPA control is allowed. This changes amplifier band context, so it stays disabled by default.

Set `kat500.follow_flex_frequency: true` only in real operational profiles where KAT tune control is intentionally enabled. When enabled, EGB sends `F <kHz>;` whenever the Flex TX frequency changes and the KAT is not already tuning, then avoids resending the same frequency before `T;`.

## Operational Mode

Normal profiles stay read-only or dry-run by default. For real AetherSDR Tune/Standby testing, use `config.aethersdr-compatible-operational.yaml`. It enables KAT500 Tune and KPA500 Standby, while keeping KPA500 Operate and clear fault disabled.

```yaml
operational:
  enable_real_controls: true
  enable_kat_tune: true
  enable_kat_bypass: false
  enable_kat_antenna: false
  enable_kpa_standby: true
  enable_kpa_operate: false
  enable_clear_fault: false
  persist_rf_risk: false
  confirm_real_hardware_control: "I understand"
```

When the confirmation string is present, operational mode can override device `dry_run` for the explicitly enabled control paths. `enable_kat_tune` allows the AetherSDR TGXL `autotune` command to send KAT500 `T;`. `enable_kpa_standby` allows KPA500 `^OS0;`. `enable_kpa_operate` allows `^OS1;` and should remain off until RF-risk testing is deliberate and local.

## Logging

```yaml
logging:
  level: info
  protocol_trace: false
  protocol_transcript_dir:
  serial_transcript_dir:
  transcript_rotate_bytes: 1048576
```

Use `debug` for internal diagnostic logs.

Set `protocol_trace: true` to print raw PGXL/TGXL lines with direction markers:

```text
PGXL RX < C1|status
PGXL TX > R1|0|...
TGXL RX < C1|status
TGXL TX > R1|0|...
```

Set `protocol_transcript_dir` to write one timestamped transcript file per client session.

Set `serial_transcript_dir` to write one timestamped KPA500/KAT500 serial transcript per device session. Transcript write failures are logged and then disabled for that session so polling is not blocked.

Set `transcript_rotate_bytes` to cap each serial or protocol transcript file. When the limit is reached, EGB opens the next indexed file for the same device/client session.

## Metrics

```yaml
metrics:
  enabled: true
  bind_ip: 127.0.0.1
  port: 9160
```

When enabled, `GET /status` returns localhost-only JSON with connection states, poll timestamps, firmware/capability fields, protocol counters, and client counts. The endpoint refuses non-loopback binds.

Phase 14 adds serial runtime counters to `/status`: poll successes/failures, reconnects, stale-state transitions, last/average/max poll latency, and stale duration.

## Flex Amplifier Injection

```yaml
flex_injection:
  enabled: false
  radio_ip: 127.0.0.1
  radio_port: 4992
  amplifier_ip: 127.0.0.1
  force_advertised_pgxl_ip:
  amplifier_port: 9008
  amplifier_model: PowerGeniusXL
  serial: EGB-KPA500
  handle: amp_1
  ant_map: ANT1:PORTA,ANT2:PORTB
  full_pgxl_registration: true
  create_meters: true
  create_interlock: true
  disable_amp_interlock: false
  amplifier_status_profile: pgxl_paired
  pgxl_force_operate_advertisement: false
  flex_force_operate_via_radio: false
  pgxl_connect_assist: false
  amplifier_reannounce_interval_ms: 5000
  pgxl_startup_trigger_strategy: current
  reconnect_initial_ms: 1000
  reconnect_max_ms: 30000
  ping_interval_ms: 30000
  tuner_refresh_interval_ms: 5000
```

Phase 19 `flex_injection` is a LAN/local-only registration client. It connects to the Flex radio TCP API on port `4992` and sends the documented PGXL-style amplifier registration sequence so the radio can advertise a `PowerGeniusXL`-compatible amplifier to AetherSDR/SmartSDR.

`amplifier_ip` must be the Windows bridge IP that macOS AetherSDR can reach. AetherSDR should use that IP for its direct PGXL connection on `amplifier_port`.

`force_advertised_pgxl_ip` overrides only the PGXL IP advertised to the radio/client. Leave it empty to advertise `amplifier_ip`. Use it for loopback-vs-LAN pairing tests, for example comparing `127.0.0.1`, the Windows LAN IP, and a VPN/Tailscale IP.

The advertised PGXL IP must match how the PGXL listener is reachable from
AetherSDR. For same-host Windows AetherSDR, bind EGB to `127.0.0.1` and
advertise `127.0.0.1`. For a remote or LAN AetherSDR client, bind EGB to the
Windows LAN IP and advertise that same LAN IP. Binding the listener to loopback
while advertising a LAN IP prevents AetherSDR's Flex-triggered PGXL auto-open
from reaching EGB and causes the delayed manual/fallback connection behaviour.

The configured `handle` is an EGB log/config label. The real Flex amplifier object handle is assigned by the radio and observed by AetherSDR through normal radio status.

`full_pgxl_registration` enables the amplifier create, AMP meter create, AMP interlock create, keepalive, subscription, and periodic ping sequence. Disable `create_meters` or `create_interlock` only for protocol isolation tests.

`disable_amp_interlock: true` is a test-only override. It skips AMP interlock creation to isolate whether SmartSDR/AetherSDR TX failures are caused by the virtual PGXL interlock. It does not send KPA500 operate and must not be used for normal operation.

`amplifier_status_profile` controls PGXL trigger experiments. `official_pgxl`, `minimal`, `pgxl_paired`, and `strict_real_pgxl` stay conservative and do not add non-standard fields to `amplifier create`; `config.aethersdr-real-operational.yaml` uses `official_pgxl` for strict protocol audit work. `aethersdr_minimal` adds only `state=<live-kpa-state>` to the create command. `aethersdr_operational` is retained as an alias for the same minimal behavior. `aethersdr_force_direct` is the locked last-known-good AetherSDR regression baseline and is allowed for `config.aethersdr-last-known-good-*.yaml`. `pgxl_verbose`, `old_good_pgxl`, and `aethersdr_pgxl_direct_lab` remain lab-only profiles for operational/evidence runs. No normal profile may hard-code `state=STANDBY`; amplifier status must follow the live KPA500 shared state.

`pgxl_force_operate_advertisement` is a lab-only switch. It advertises `state=OPERATE` to AetherSDR without sending any KPA500 command. Use it only to determine whether AetherSDR refuses to open TCP 9008 while the injected amplifier is in `STANDBY`.

`flex_force_operate_via_radio` is also lab-only. It sends `amplifier set <handle> operate=1` to the Flex API after the injected amplifier handle is observed. It does not send `^OS1;` to the KPA500 and is used only to test whether the Flex radio owns and rewrites amplifier operate state.

`pgxl_connect_assist` is a lab-only AetherSDR PGXL direct-connect workaround. It sends one Flex-side `amplifier set <handle> operate=1` after the virtual amplifier handle is discovered, which can trigger AetherSDR to open TCP 9008. It does not send `^OS1;` to the KPA500; PGXL direct status remains based on real KPA500 polling. This is not normal lifecycle machinery and must stay disabled for operational/evidence runs.

`pgxl_startup_trigger_strategy` controls only the bounded startup burst used to measure PGXL direct-connect timing. `current` preserves the last-known-good behaviour, `rapid_sub_only` sends faster `sub amplifier all` refreshes, `reannounce_status_only` logs/status-reannounces without subscription spam, `reannounce_create_style_status` replays the create-style line into evidence for comparison, and `no_burst` disables the startup burst. Use `egb pgxl-trigger-strategy-test` to compare these; do not change the working AetherSDR profile while measuring.

`aethersdr_open_trigger_variant` is a Phase 62/69 lab-only field used by `egb aethersdr-open-trigger-test`. It varies only amplifier advertisement/create fields to prove what makes AetherSDR open PGXL TCP and control-capable UI. Valid values are `current`, `no_hack_fields`, `state_only`, `state_connected`, `state_ip_port`, `state_model_ip_port_serial`, `availability_fields`, and `current_hack_fields`. Use `no_hack_fields`, `state_only`, and `current_hack_fields` for isolated comparisons of the old readiness fields.

Set `flex_injection.trace_amplifier_advertisements: true` while debugging PGXL pairing. EGB writes every emitted amplifier create/status advertisement to `logs/flex/amplifier-advertisements.jsonl` and to the active evidence bundle.

`enable_runtime_interlock` enables the Flex Ethernet AMP interlock runtime loop. When Flex reports `state=PTT_REQUESTED`, EGB sends `interlock ready <id>` only if RF-risk permission, KPA connectivity, OPERATE state, and fault checks all pass; otherwise it sends `interlock not_ready <id>` and logs the reason. This is off by default except in explicit SmartSDR/Flex runtime test profiles.

`enable_vita_meter_publish` enables the experimental VITA-49 AMP meter publisher. EGB sends externally-created AMP meter values to the radio UDP VITA port `4991` using meter IDs and stream IDs returned by `meter create`. This is off by default until live SmartSDR meter display is verified.

`allow_mismatched_advertised_ip` is a lab-only override. Operational/evidence starts now fail if `server.bind_ip` is loopback while the advertised PGXL IP is a LAN address, because AetherSDR opens PGXL TCP using the advertised amplifier IP. Use `config.aethersdr-production.yaml` for same-PC AetherSDR (`127.0.0.1` bind and advertised IP), and `config.smartsdr-pgxl-meter-test.yaml` for LAN SmartSDR meter/interlock experiments.

`flex_injection.amplifier_startup_state_policy` controls whether PGXL/Flex amplifier advertisement can happen before KPA500 telemetry is real:

- `wait_for_first_kpa_poll`: recommended for real hardware. EGB waits for `^OS;`, `^TM;`, `^VI;`, and `^FL;` before sending the direct-connect amplifier create with live state.
- `advertise_standby_immediately`: lab-only compatibility mode.
- `advertise_configured_default`: reserved for future explicit default-state profiles.

If first poll does not complete before `wait_first_kpa_poll_timeout_ms`, EGB emits `kpa500_not_polling` with the COM port/open error and proceeds in degraded mode.

`amplifier_reannounce_interval_ms` controls the rate-limited amplifier refresh query and evidence logging. It does not create duplicate amplifier objects.

This mode does not implement live radio-side meter value streaming, proxying, TLS, or WAN exposure. Operate remains RF-risk gated by `kpa500.allow_rf_risk`.

## Soak And Stability Tests

Use soak mode for long-duration validation:

```powershell
cargo run -p egb -- soak-test --config config.hardware-readonly.yaml --duration-hours 4
```

Use evidence mode for a bounded SmartSDR/Flex reconnect capture:

```powershell
.\target-msvc\debug\egb.exe evidence-test --config .\config.flex-injection-readonly.yaml --duration-minutes 10
```

Both start the normal bridge runtime. `evidence-test` also writes a complete evidence folder and ZIP to `diagnostics\runs`. See `docs/evidence-bundles.md`.

## Mock Fault Simulation

```yaml
mock:
  pgxl_fault: false
  tgxl_fault: false
  high_swr: false
```

These flags only affect mock-mode state. Use them later for degraded UI testing, not for first applet activation tests.

## Control Verification

```yaml
control:
  verify_delay_ms: 200
```

No-ACK control commands use this delay before a follow-up verification query. KPA500 standby sends `^OS0;`, waits, then verifies with `^OS;` expecting `^OS0;`.

## Profiles

- `config.mock.yaml`: no hardware required, protocol trace enabled, strict startup simulation and TGXL direct-presence diagnostics enabled.
- `config.hardware-readonly.yaml`: COM8/COM21 hardware mode with `dry_run: true`.
- `config.aethersdr-compat-readonly.yaml`: COM8/COM21 hardware mode with `dry_run: true` and compatibility response formatting enabled for AetherSDR disconnect/app visibility testing.
- `config.flex-injection-readonly.yaml`: COM8/COM21 hardware mode with `dry_run: true`, AetherSDR compatibility formatting, and passive Flex amplifier registration enabled. Edit `flex_injection.radio_ip` and `flex_injection.amplifier_ip` before use.
- `config.hardware-control-local-only.yaml`: COM21 KPA500 control mode with `dry_run: false`, COM8 KAT500 still `dry_run: true`, loopback bind by default. Use only locally or on a private LAN after read-only validation.

# SmartSDR TGXL Stability

Observed Phase 21 state:

- SmartSDR sees and connects to the tuner.
- After a while SmartSDR loses the tuner connection and reconnects.
- KAT500 dashboard polling is stable.

## Added Instrumentation

The TGXL direct TCP server now records active sessions in `/status`:

```text
clients.tgxl_sessions[]
clients.tgxl_last_disconnect_reason
flex_diagnostics.ping_count
flex_diagnostics.ping_failures
flex_diagnostics.pending_count
flex_diagnostics.expired_pending_count
flex_diagnostics.degraded_reason
```

Each session includes:

- session id
- peer address
- connected timestamp
- last command
- command/response counters
- parse failure count
- unknown command count
- last and max response latency

The disconnect log also includes the normal session summary.

Phase 22 adds `tgxl.smartsdr_compat: true`. It uses the same conservative direct TGXL formatting as AetherSDR compatibility mode: source-observed fields only and no experimental direct status fields.

## Current Suspected Causes

Most likely causes to validate with a diagnostics ZIP:

- SmartSDR expects a TGXL keepalive/status cadence different from AetherSDR.
- SmartSDR opens more than one tuner client and one session ages out.
- A response field is tolerated initially but rejected after a state transition.
- Flex-side tuner presence refresh is incomplete or ages out.

## Next Capture

Run a 10-minute stability capture, reproduce the SmartSDR reconnect if possible, then export diagnostics:

```powershell
.\target-msvc\debug\egb.exe evidence-test --config .\config.flex-injection-readonly.yaml --duration-minutes 10
```

Check:

- `clients.tgxl_sessions`
- `clients.tgxl_last_disconnect_reason`
- TGXL protocol transcript around the disconnect
- Flex command responses and ping count
- `logs\tests`
- Windows Event Viewer crash entry if SmartSDR crashes

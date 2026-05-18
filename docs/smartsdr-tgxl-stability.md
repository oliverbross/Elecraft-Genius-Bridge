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

## Current Suspected Causes

Most likely causes to validate with a diagnostics ZIP:

- SmartSDR expects a TGXL keepalive/status cadence different from AetherSDR.
- SmartSDR opens more than one tuner client and one session ages out.
- A response field is tolerated initially but rejected after a state transition.
- Flex-side tuner presence refresh is incomplete or ages out.

## Next Capture

Run the GUI, start the bridge, reproduce the SmartSDR reconnect, then export diagnostics. Check:

- `clients.tgxl_sessions`
- `clients.tgxl_last_disconnect_reason`
- TGXL protocol transcript around the disconnect
- Flex command responses and ping count

# Controlled Operate Design

This is a design note only. Phase 14 does not enable RF-risk operate testing.

## Goal

Allow a future LAN-only KPA500 operate test with explicit operator intent, clear rollback, and no accidental WAN exposure.

## Preconditions

- Bridge bind address is loopback or private LAN only.
- Public port forwarding is disabled.
- KPA500 read-only polling is stable for a soak period.
- KPA500 standby `^OS0;` remains verified by post-query `^OS; -> ^OS0;`.
- Operator can see the physical station state.
- Exciter RF is inhibited or power is reduced to a safe level.
- Dummy load is connected before any RF-producing test.

## Proposed Workflow

1. Start from `config.hardware-control-local-only.yaml`.
2. Confirm `server.bind_ip` is `127.0.0.1` or a specific private LAN IP.
3. Run read-only tests immediately before control:

   ```powershell
   cargo run -p egb -- test-kpa --config config.hardware-readonly.yaml
   cargo run -p egb -- test-kat --config config.hardware-readonly.yaml
   ```

4. Send standby first and verify:

   ```powershell
   cargo run -p egb -- test-kpa --config config.hardware-control-local-only.yaml --allow-control
   ```

5. Only in a future phase, add a separate operate command path requiring `--allow-rf-risk`.
6. After any operate attempt, force standby with `^OS0;` and verify with `^OS;`.

## Rollback

- Send `^OS0;`.
- Verify `^OS; -> ^OS0;`.
- Stop EGB.
- Remove LAN inbound firewall rules if any were temporarily added.
- Power-cycle the amp only if serial state and front panel disagree.

## RF Inhibit Options To Validate

- Radio transmit inhibit, if available.
- Drive power set to zero or minimum.
- Dummy load connected.
- No antenna selected for RF output during first operate-only test.

## Still Blocked

- KPA500 `^OS1;` operate is RF-risk and remains disabled unless `--allow-rf-risk` is explicitly present.
- KAT500 tune, bypass, and antenna control remain blocked.
- WAN operation remains blocked by design until authentication/TLS/deployment hardening exists.

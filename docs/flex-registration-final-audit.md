# Flex Registration Final Audit

This audit covers the Flex amplifier side of the PGXL emulation after the Phase
65 review.

## Intended Sequence

For one Flex TCP session, EGB must:

1. Connect to the radio API port.
2. Receive the client handle (`H...`).
3. Send exactly one amplifier create for the session.
4. Continue registration even if no amplifier status handle is broadcast
   immediately after create.
5. Create AMP meters `FWD`, `RL`, `DRV`, `ID`, and `TEMP`.
6. Create the AMP interlock unless the explicit interlock-disabled test mode is
   active.
7. Enable keepalive.
8. Subscribe to amplifier, slice, and TX status.
9. Ping periodically.
10. Reannounce state changes using subscriptions/status refreshes, not duplicate
    amplifier creates.

## Latest Live Behaviour

Recent evidence before this phase showed:

- Flex API connected.
- Amplifier handle exists.
- Meter handles exist.
- Interlock handle exists.
- `amplifier_removed_count=0`.
- No handle churn.
- KPA/KAT polling stable.

The remaining PGXL open delay was not caused by amplifier removal or handle
churn. It was caused by the advertised PGXL IP not matching the bound listener
address for same-host AetherSDR.

## Addressing Rule

The advertised amplifier IP is consumed by AetherSDR, not by the Flex radio
itself. Therefore:

- Same-host AetherSDR: bind PGXL to loopback and advertise `127.0.0.1`.
- LAN AetherSDR: bind PGXL to the Windows LAN IP and advertise that same LAN IP.
- Do not bind to loopback while advertising a LAN IP.

EGB now logs `PGXL_ADVERTISED_IP_UNREACHABLE_FROM_LOOPBACK_BIND` when this
misconfiguration is detected at startup.

## Create Profile

`aethersdr_force_direct` remains the AetherSDR regression baseline. It may carry
AetherSDR compatibility fields that strict Flex documentation does not require,
but it is retained because it preserves the observed AetherSDR widget path.

The strict audit profile remains available:

`amplifier create ip=<ip> port=9008 model=PowerGeniusXL serial_num=<serial> ant=ANT1:PORTA,ANT2:PORTB`

## Reannounce Policy

Telemetry changes do not recreate objects.

KPA state changes now request an immediate bounded refresh burst:

- `kpa_state_changed`: four quick `sub amplifier all` refreshes at 250 ms cadence.
- `kpa_telemetry_changed`: one quick refresh.

This is intentionally bounded. It improves AetherSDR state reflection without
reintroducing registration churn.

## Remaining Flex Gaps

- Meter objects are created, but client-side meter value publication is not
  verified. SmartSDR PGXL can see the object but does not receive full live data
  from EGB yet.
- Flex-side TGXL/tuner registration is not implemented because no verified public
  tuner registration command has been proven for this device class.

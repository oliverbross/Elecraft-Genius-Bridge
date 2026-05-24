# Flex Control Capability Audit

Phase 68 reviewed the Flex-side amplifier object because both AetherSDR and
SmartSDR can display the virtual PGXL while still behaving read-only.

## Current Registration

EGB registers one PGXL-style amplifier object per Flex TCP session:

`amplifier create ip=<egb-ip> port=9008 model=PowerGeniusXL serial_num=<serial> ant=ANT1:PORTA,ANT2:PORTB ...`

The AetherSDR compatibility profile may include `state=<live-kpa-state>` at
create time because that was the observed direct-open trigger. The official
audit profile omits that field.

Post-create registration sends:

- `meter create name=FWD type=AMP min=30.0 max=63.01 units=DBM`
- `meter create name=RL type=AMP min=34.0 max=60.0 units=DB`
- `meter create name=DRV type=AMP min=10.0 max=50.00 units=DBM`
- `meter create name=ID type=AMP min=0.0 max=70.0 units=AMPS`
- `meter create name=TEMP type=AMP min=0.0 max=100.0 units=TEMP_C`
- `interlock create type=AMP valid_antennas=ANT1,ANT2 name=PG-XL serial=<serial>`
- `keepalive enable`
- `sub amplifier all`
- `sub slice all`
- `sub tx all`
- periodic `ping`

The operational success criteria are: one create per Flex session, no amplifier
removed event, stable amplifier handle, meters/interlock accepted when enabled,
and no duplicate create churn.

## Control Semantics

AetherSDR source shows the AMP applet button sends:

`amplifier set <handle> operate=<0|1>`

through the Flex radio API. It does not use direct PGXL TCP for standby/operate.
The direct PGXL TCP path in AetherSDR polls `info` and `status`.

EGB accepts two possible Flex-side control indications:

1. Explicit `operate=0` / `operate=1` on an amplifier status line.
2. External-client-originated `state=STANDBY` / `state=OPERATE` amplifier
   status where the status source handle is not EGB's own Flex client handle.

The second path is intentionally guarded so EGB does not treat its own reflected
KPA telemetry as a command. Operate remains RF-risk gated; standby remains
allowed only when the effective control policy allows it.

## Meter Semantics

Meter object creation is not enough to populate SmartSDR PGXL meters. A public
FlexRadio community answer from a FlexRadio administrator says external meter
data on LAN is sent as UDP VITA-49 meter packets to radio port `4991`, and the
SmartSDR API docs describe meter values as streaming data separate from TCP
status. EGB currently creates and tracks AMP meter handles but does not publish
VITA-49 AMP meter packets.

Operational result:

- AetherSDR gets live PGXL data from direct TCP status.
- SmartSDR PGXL object visibility can work.
- SmartSDR live PGXL meter data remains partial until EGB implements verified
  VITA-49 AMP meter publication.

## Interlock Semantics

EGB creates the AMP interlock unless explicit test mode disables it. Empty
`amplifier=` on a Flex interlock status is warning-level when `tx_allowed=1` and
blocking when `tx_allowed=0`.

The interlock status is not currently proven to gate AetherSDR AMP button command
emission: interlock-disabled test runs still showed no command arriving at EGB.

## Current Gap

The remaining read-only behaviour is not caused by KPA/KAT serial transport.
It is Flex-side/client-side:

- If no `amplifier set` command or external-client amplifier `state=` status
  reaches EGB, EGB cannot change KPA500 state.
- If SmartSDR expects live AMP meters, EGB must publish VITA-49 meter values,
  not just create meter objects.
- If SmartSDR expects a TGXL/tuner object, EGB needs a verified Flex-side tuner
  registration sequence or a real TGXL capture.

## Phase 69 Re-Audit

The SmartSDR interlock API shows that a dynamic Ethernet AMP interlock is not
only a registration record. The external device is expected to participate in a
PTT-time `PTT_REQUESTED` -> `interlock ready <id>` exchange. EGB currently
creates the PGXL interlock and records `state`/`tx_allowed`, but it does not yet
perform the ready/not-ready exchange. This is a SmartSDR TX/interlock gap and a
possible "partially operable" signal to clients.

The SmartSDR meter API also confirms that externally created meters can receive
values through UDP VITA-49 packets on port `4991`. EGB's prior
`meter_publish_supported=false` should be read as an implementation gap, not as
an API impossibility.

Phase 69 adds isolated advertisement variants:

- `no_hack_fields`
- `state_only`
- `current_hack_fields`

These variants allow live comparison of strict/no-hack, minimal state, and old
readiness-field behaviour without changing the working default profile.

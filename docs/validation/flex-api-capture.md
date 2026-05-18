# Flex API Capture Workflow

Use this when validating Phase 19 PGXL radio-side registration with AetherSDR or SmartSDR.

## What To Capture

Capture three streams:

1. Flex discovery packets, if radio discovery is under investigation.
2. EGB -> Flex radio TCP API on port `4992`.
3. AetherSDR -> Flex radio TCP API on port `4992`.

The most important evidence is whether AetherSDR receives:

```text
S...|amplifier <handle> model=PowerGeniusXL ip=<egb-ip> port=9008 ...
```

after EGB sends:

```text
C1|amplifier create ip=<egb-ip> port=9008 model=PowerGeniusXL serial_num=EGB-KPA500 ant=ANT1:PORTA,ANT2:PORTB
C2|meter create name=FWD type=AMP min=30.0 max=63.01 units=DBM
C3|meter create name=RL type=AMP min=34.0 max=60.0 units=DB
C4|meter create name=DRV type=AMP min=10.0 max=50.00 units=DBM
C5|meter create name=ID type=AMP min=0.0 max=70.0 units=AMPS
C6|meter create name=TEMP type=AMP min=0.0 max=100.0 units=TEMP_C
C7|interlock create type=AMP valid_antennas=ANT1,ANT2 name=PG-XL serial=EGB-KPA500
C8|keepalive enable
C9|sub amplifier all
```

## Windows Capture

Install Wireshark/Npcap on the Windows bridge host.

Display filter:

```text
tcp.port == 4992 || tcp.port == 9008 || tcp.port == 9010 || udp.port == 4992 || udp.port == 4991
```

Suggested capture order:

1. Start Wireshark capture.
2. Start EGB with `config.flex-injection-readonly.yaml`.
3. Start AetherSDR and connect to the Flex radio.
4. Connect AetherSDR PGXL/TGXL direct IPs if not automatic.
5. Save the capture as `docs/packet-captures/flex-injection/YYYYMMDD-aethersdr-pa.pcapng`.

## EGB Logs To Save

Run with `logging.level: debug`.

Save lines containing:

```text
Flex amplifier injection connecting
Flex API version received
Flex API client handle received
FLEX TX >
FLEX RX <
Flex PGXL registration command sent
Flex PGXL registration command accepted
```

## Expected Good Sequence

```text
FLEX RX < V...
FLEX RX < H...
FLEX TX > C1|amplifier create ip=... port=9008 model=PowerGeniusXL serial_num=EGB-KPA500 ant=ANT1:PORTA,ANT2:PORTB
FLEX RX < R1|0...
FLEX TX > C2|meter create name=FWD type=AMP min=30.0 max=63.01 units=DBM
FLEX RX < R2|0...
FLEX TX > C7|interlock create type=AMP valid_antennas=ANT1,ANT2 name=PG-XL serial=EGB-KPA500
FLEX RX < R7|0...
FLEX TX > C8|keepalive enable
FLEX RX < R8|0...
```

Then, in the AetherSDR Flex API stream:

```text
C...|sub amplifier all
R...|0|
S...|amplifier <handle> model=PowerGeniusXL ip=... port=9008 ...
```

## Failure Cases

`R1|50000016` or malformed command:

- Check `ant_map`, `serial`, and `amplifier_model` for spaces or invalid separators.

EGB registration accepted but no AetherSDR PA applet:

- Confirm AetherSDR receives the radio-originated amplifier status.
- Confirm AetherSDR source/binary still uses the amplifier status path documented in `docs/analysis/aethersdr-ui-activation-phase16.md`.

Registration accepted but AetherSDR does not connect to PGXL:

- Confirm `flex_injection.amplifier_ip` is the Windows bridge LAN IP reachable from macOS.
- Confirm EGB is listening on `pgxl.port`.

Registration not accepted:

- Save the `R1|...` response and Flex firmware version.
- Do not add proxy mode until the exact radio response is understood.

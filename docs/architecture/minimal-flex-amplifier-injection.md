# Minimal Flex Amplifier-Status Injection

Status: design only. Do not implement until direct TGXL binary/layout validation is complete.

## Purpose

Make stock AetherSDR instantiate PA/AMP and, if needed, TUN applets when the real Flex radio does not advertise PGXL/TGXL accessories but EGB has working direct PGXL/TGXL sockets and Elecraft-backed state.

## Source Requirement

The inspected AetherSDR code shows:

- `AMP` visibility requires `RadioModel::amplifierChanged(true)`.
- `RadioModel::amplifierChanged(true)` is emitted only after a Flex API status line with object `amplifier <handle>` and non-empty `model` other than `TunerGeniusXL`.
- `TUN` can appear from direct TGXL TCP in current source, but older binaries may still need a radio-side `model=TunerGeniusXL` record.

## Minimal Topology

```text
AetherSDR
  -> EGB Flex API proxy
      -> real Flex radio TCP API

AetherSDR
  -> EGB PGXL emulator :9008
  -> EGB TGXL emulator :9010

EGB
  -> KPA500 COM21
  -> KAT500 COM8
```

## Passive Injection Candidate

After the client subscribes to amplifier status, inject synthetic status lines using valid hexadecimal handles that should not collide with radio-generated handles:

```text
S<radio>|amplifier 0x7E500001 model=PowerGeniusXL ip=<egb-ip> state=STANDBY serial_num=EGB-PGXL temp=<c> peakfwd=<dbm> swr=<rl-db>
S<radio>|amplifier 0x7E500002 model=TunerGeniusXL ip=<egb-ip> operate=0 bypass=<0|1> tuning=0 relayC1=<n> relayL=<n> relayC2=<n> antA=<n> one_by_three=1 serial_num=EGB-TGXL
```

Notes:

- Handles must be valid hex because AetherSDR converts TGXL handles with `toUInt(nullptr, 0)`.
- Injected state must derive from shared state, including degraded/stale state.
- Start with PGXL only if TGXL direct presence is proven in the installed binary.
- Remove candidate fields if AetherSDR rejects them; update replay tests first.

## Command Handling

Passive injection can make the applets visible, but controls may then send radio-side commands against synthetic handles:

```text
amplifier set 0x7E500001 operate=0
amplifier set 0x7E500001 operate=1
tgxl set handle=0x7E500002 mode=<0|1>
tgxl set handle=0x7E500002 bypass=<0|1>
tgxl autotune handle=0x7E500002
```

Initial implementation should intercept only safe commands:

- `operate=0` maps to verified KPA500 standby `^OS0;`.
- `operate=1` remains blocked unless `--allow-rf-risk` and future RF-risk gates are explicitly enabled.
- KAT500 tune/bypass/antenna changes remain blocked until control mappings are verified.

The proxy must synthesize a Flex API command response only for commands it intercepts. All other client-to-radio traffic must pass through unchanged.

## Injection Timing

Candidate sequence:

1. Forward AetherSDR connection to the real radio.
2. Observe the radio handle from the normal `H<handle>` line.
3. Observe or forward `sub amplifier all`.
4. After the radio's subscription response/status burst, inject synthetic `S<radio>|amplifier ...` lines.
5. Re-inject on reconnect and when state transitions.
6. Inject `removed=1` for synthetic handles on proxy shutdown or when EGB disables the device.

## Risks

- The proxy becomes part of the primary radio control path.
- SmartLink is not a plain LAN TCP session and is not solved by this design.
- Synthetic handles can conflict if chosen poorly.
- Incorrect response sequencing can destabilize AetherSDR.
- Stock WAN exposure of the proxy would expose the Flex radio control plane.

## Recommendation

Use this path only if:

- Oliver needs stock AetherSDR without a custom patch, and
- the Flex radio cannot natively advertise PGXL/TGXL presence, and
- layout reset/binary verification confirms direct TGXL behavior is not enough for the target app.

For a custom AetherSDR build, adding PGXL direct-presence support analogous to TGXL is the lower-risk engineering path.

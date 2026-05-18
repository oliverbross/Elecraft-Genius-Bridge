# AetherSDR Disconnect Analysis

Date: 2026-05-18

## Observed Session

Recent protocol transcripts:

- `logs/protocol/tgxl-1779072204686-127_0_0_1_50673-0.log`
- `logs/protocol/pgxl-1779072204701-127_0_0_1_50674-0.log`

TGXL startup sequence:

```text
TGXL TX > V0.1.0-egb-tgxl
TGXL RX < C1|info
TGXL TX > R1|0|model=TunerGeniusXL serial_num=EGB-TGXL version=0.1.0-egb-tgxl firmware=0.1.0-egb-tgxl one_by_three=1 capabilities=direct_tcp,status,autotune,ant,manual_tune
TGXL RX < C2|status
TGXL TX > R2|0|operate=1 bypass=0 tuning=0 relayC1=0 relayL=0 relayC2=0 antA=2 one_by_three=1 fwd=0.0000 swr=1.1400 connection_state=connected fault=
```

The server-side transcript ends after a normal response line. There is no malformed outbound frame or explicit EGB-side close in the transcript. Windows socket error `10053` therefore looks like the client aborted the connection or the local TCP stack closed it after AetherSDR decided the session was no longer usable.

## Source Comparison

Inspected source:

- `research/AetherSDR/src/core/TgxlConnection.cpp`
- `research/AetherSDR/src/models/TunerModel.cpp`
- `research/AetherSDR/src/core/PgxlConnection.cpp`
- `research/AetherSDR/src/gui/MainWindow.cpp`

AetherSDR direct TGXL response parsing accepts arbitrary `key=value` tokens, but the model only consumes a subset from direct `statusUpdated`:

- `antA`
- `fwd`
- `swr`

For direct TGXL `stateUpdated`, it consumes:

- `relayC1`
- `relayL`
- `relayC2`
- `antA`
- `fwd`
- `swr`

Radio-side amplifier status, not direct TGXL status, consumes:

- `operate`
- `bypass`
- `tuning`
- `model`
- `serial_num`
- `one_by_three`
- `ip`

## Suspected Problems

1. `swr` was encoded as an SWR ratio.

   AetherSDR source treats direct TGXL `swr` as return loss in dB and converts it to an SWR ratio. Sending `swr=1.1100` or `swr=1.1400` is semantically wrong for that code path and can produce an extreme UI value. EGB now emits negative return loss, using `-30.0000` for the no-RF / 1:1 baseline.

2. Status responses were delayed by shared-state lock contention.

   The KPA500/KAT500 serial polling loops updated shared state and then slept while still holding the write lock. During real hardware polling, that could block PGXL/TGXL response generation. The transcript showed approximately two seconds between `C<N>|status` and `R<N>|...`. EGB now drops the write lock before sleeping between serial polls.

3. `operate=1` is suspicious for direct TGXL read-only mode.

   In AetherSDR source, `operate` is meaningful in radio-side amplifier status. Direct TGXL status does not use it for state. In compatibility mode, EGB reports `operate=0` to avoid implying an active tuner control mode during read-only tests.

4. Extra fields were unverified.

   `firmware`, `capabilities`, `connection_state`, and `fault` are harmless according to the key/value parser, but they are not known real PGXL/TGXL direct fields. Compatibility mode removes them.

## Compatibility Mode Changes

When `aethersdr_compat: true`:

PGXL `info`:

```text
model=PowerGeniusXL serial_num=EGB-PGXL version=0.1.0-egb-pgxl
```

PGXL `status`:

```text
state=STANDBY peakfwd=0.0000 swr=-30.0000 temp=30.0 id=0.0 vac=0 meffa=UNKNOWN
```

TGXL `info`:

```text
model=TunerGeniusXL serial_num=EGB-TGXL version=0.1.0-egb-tgxl one_by_three=1
```

TGXL `status`:

```text
operate=0 bypass=0 tuning=0 relayC1=0 relayL=0 relayC2=0 antA=2 one_by_three=1 fwd=0.0000 swr=-25.6
```

The exact `swr` value depends on the real KAT500 SWR ratio converted to return loss. No-RF / 1.0 maps to `-30.0000`.

## Applet Visibility

The direct sockets being connected still does not prove that PA/ATU applets will be visible. The inspected AetherSDR source shows:

- TGXL TUN visibility can use `TunerModel::m_directPresence` in newer source.
- PGXL AMP visibility still appears tied to radio-side amplifier presence.

If compatibility mode stabilizes TGXL but applets remain hidden, the next decision remains whether the tested AetherSDR binary includes the direct-presence patch and whether PGXL requires Flex API amplifier-status injection or an AetherSDR change.

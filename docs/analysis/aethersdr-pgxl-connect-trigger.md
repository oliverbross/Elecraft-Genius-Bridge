# AetherSDR PGXL Connect Trigger

Inspected AetherSDR source revision:

```text
6d17b3bbda96b836762e7d40758a1fc3e14725f9
```

## Direct TCP Trigger

Automatic PGXL direct TCP is opened from `src/gui/MainWindow.cpp`:

```cpp
connect(&m_radioModel, &RadioModel::amplifierChanged, this, [this](bool present) {
    if (present && !m_radioModel.ampIp().isEmpty() && !m_pgxlConn.isConnected()) {
        m_pgxlConn.connectToPgxl(m_radioModel.ampIp());
    } else if (!present) {
        m_pgxlConn.disconnect();
    }
});
```

The required condition is:

- `RadioModel::amplifierChanged(true)` fires.
- `RadioModel::ampIp()` is non-empty.
- `PgxlConnection` is not already connected.

`RadioModel::ampIp()` is populated in `src/models/RadioModel.cpp` when a Flex status line with object `amplifier <handle>` contains a non-empty `model` that is not `TunerGeniusXL`:

```cpp
if (!model.isEmpty() && model != "TunerGeniusXL") {
    m_ampHandle = handle;
    if (!m_hasAmplifier) {
        m_hasAmplifier = true;
        m_ampIp = kvs.value("ip");
        m_ampModel = model;
        emit amplifierChanged(true);
    }
}
```

Therefore, for AetherSDR auto-connect, the radio-side amplifier status must include at least:

```text
S...|amplifier <handle> model=PowerGeniusXL ip=<egb-ip> ...
```

`port=9008` is useful evidence but is not used by the automatic trigger in the inspected source; auto-connect calls `connectToPgxl(m_radioModel.ampIp())`, which defaults to port `9008`.

## Manual IP Trigger

Manual PGXL connection is independent. `src/gui/RadioSetupDialog.cpp` persists:

```text
PGXL_ManualIp
PGXL_ManualPort
```

`src/gui/MainWindow.cpp` also auto-connects saved manual peripherals after radio startup:

```cpp
QString pgxlIp = cs.value("PGXL_ManualIp", "").toString();
if (!pgxlIp.isEmpty() && !m_pgxlConn.isConnected()) {
    quint16 pgxlPort = static_cast<quint16>(cs.value("PGXL_ManualPort", "9008").toInt());
    m_pgxlConn.connectToPgxl(pgxlIp, pgxlPort);
}
```

## Why TGXL Opens But PGXL Does Not

TGXL has two paths:

- radio-side `amplifier <handle> model=TunerGeniusXL ...`
- direct TCP presence fallback in `TunerModel::m_directPresence`

PGXL does not have an equivalent direct-presence fallback in the inspected source. It needs either a saved manual PGXL IP or a radio-side power-amplifier status with `ip=<egb-ip>`.

If EGB shows an amplifier handle but AetherSDR never opens TCP `9008`, the likely causes are:

- the status seen by AetherSDR did not include `ip=...`,
- the `amplifierChanged(true)` event fired before `m_ampIp` was populated in the running binary,
- the running AetherSDR binary differs from the inspected source,
- the IP is loopback or unreachable from the macOS client,
- manual PGXL IP settings are empty and the radio-side auto-connect status did not reach this client.

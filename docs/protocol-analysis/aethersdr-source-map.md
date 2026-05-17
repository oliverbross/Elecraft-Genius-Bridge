# AetherSDR Source Map

Research target:

- Repository: `https://github.com/ten9876/AetherSDR`
- Local clone: `research/AetherSDR`
- Commit inspected: `6d17b3bbda96b836762e7d40758a1fc3e14725f9`

## PGXL Direct TCP

| Topic | File |
| --- | --- |
| Connection object | `src/core/PgxlConnection.h` |
| TCP, handshake, parser, status poll | `src/core/PgxlConnection.cpp` |
| PGXL status consumption | `src/gui/MainWindow.cpp` |
| AMP applet state rendering | `src/gui/AmpApplet.cpp` |
| Radio-proxied amplifier status | `src/models/RadioModel.cpp` |
| Manual IP UI | `src/gui/RadioSetupDialog.cpp` |

## TGXL Direct TCP

| Topic | File |
| --- | --- |
| Connection object | `src/core/TgxlConnection.h` |
| TCP, handshake, parser, status poll | `src/core/TgxlConnection.cpp` |
| TGXL state model and commands | `src/models/TunerModel.h` |
| Direct TGXL field consumption | `src/models/TunerModel.cpp` |
| Tuner applet state rendering | `src/gui/TunerApplet.cpp` |
| Direct TGXL wiring | `src/gui/MainWindow.cpp` |
| Radio-proxied TGXL status | `src/models/RadioModel.cpp` |
| Manual IP UI | `src/gui/RadioSetupDialog.cpp` |

## Discovery

| Topic | File |
| --- | --- |
| Flex radio UDP discovery | `src/core/RadioDiscovery.h` |
| Flex radio discovery packet parsing | `src/core/RadioDiscovery.cpp` |
| Manual IP external device tab | `src/gui/RadioSetupDialog.cpp` |
| Saved manual auto-connect | `src/gui/MainWindow.cpp` |
| Antenna Genius UDP discovery, not PGXL/TGXL | `src/models/AntennaGeniusModel.*` |

## Key Conclusions

- PGXL direct connection is TCP port `9008`.
- TGXL direct connection is TCP port `9010`.
- Both are server-speaks-first: AetherSDR waits for `V<version>`.
- Both use newline-delimited `C/R/S/V` text framing.
- AetherSDR's direct PGXL/TGXL implementation does not use JSON.
- AetherSDR's direct PGXL/TGXL implementation does not include direct UDP discovery.
- Manual IP is a supported and sufficient MVP path for WAN operation.


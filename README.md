# Elecraft Genius Bridge

Elecraft Genius Bridge (EGB) is a Windows-first bridge daemon that lets AetherSDR connect to Elecraft station hardware as if it were 4O3A/Flex Genius hardware:

| Real device | Emulated network device |
| --- | --- |
| Elecraft KPA500 | Power Genius XL (PGXL) |
| Elecraft KAT500 | Tuner Genius XL (TGXL) |

Current status: MVP implementation scaffold with mock PGXL/TGXL emulators and compiling serial-driver stubs. The Elecraft command mappings are intentionally marked unverified until official command references or hardware captures are added.

## What Works In This Phase

- Rust workspace with separated crates.
- YAML configuration and validation.
- Shared state model.
- PGXL emulator on TCP `9008`.
- TGXL emulator on TCP `9010`.
- Server-first `V<version>` greeting for AetherSDR.
- Newline-delimited `C/R/S/V` parser and response framing.
- Mock KPA500/KAT500 state.
- Optional raw protocol trace logs and per-client transcript files.
- CLI commands:
  - `egb run --config config.yaml`
  - `egb check-config --config config.yaml`
  - `egb list-serial`
  - `egb test-kpa --config config.yaml`
  - `egb test-kat --config config.yaml`
  - `egb replay-pgxl`
  - `egb replay-tgxl`

## Quick Start

```powershell
Copy-Item config.example.yaml config.yaml
cargo run -p egb -- check-config --config config.yaml
cargo run -p egb -- run --config config.yaml
```

The default config binds to `127.0.0.1` for safety. Change `server.bind_ip` to your LAN IP when testing from another machine.

## AetherSDR Test

1. Start EGB in mock mode.
2. In AetherSDR, open the Peripherals/manual IP setup.
3. Configure PGXL to the bridge IP and port `9008`.
4. Configure TGXL to the bridge IP and port `9010`.
5. Confirm logs show repeated `status` polling.

## Safety

Do not port-forward raw PGXL/TGXL emulator ports to the public internet for this MVP. Authentication and TLS are reserved for a later phase. Use LAN-only testing, Tailscale, ZeroTier, VPN, or a controlled reverse proxy during development.

## Repository Layout

```text
crates/
  bridge-core/       Shared protocol and state
  config/            YAML config loading and validation
  pgxl-emulator/     PGXL direct TCP emulator
  tgxl-emulator/     TGXL direct TCP emulator
  elecraft-kpa500/   KPA500 serial driver scaffold
  elecraft-kat500/   KAT500 serial driver scaffold
  test-harness/      Replay entry points
  egb/               CLI binary
docs/
  protocol-analysis/
  architecture/
  packet-captures/
  state-machines/
```

## Verification Targets

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

In the initial Windows environment, `cargo test --workspace` passed using the installed rustup MSVC toolchain. `rustup` itself was not on `PATH`, but prepending the MSYS2 tool directory made `cargo fmt` and `cargo clippy` available to the MSVC Cargo; all three checks passed with that setup.

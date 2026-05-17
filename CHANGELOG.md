# Changelog

## Unreleased

- Added Rust workspace for Elecraft Genius Bridge.
- Added mock PGXL and TGXL direct TCP emulators for AetherSDR manual IP use.
- Added YAML configuration and validation.
- Added protocol trace logging and optional per-session transcript capture.
- Added validation checklists for AetherSDR mock testing and Elecraft hardware testing.
- Added GitHub Actions CI definition.
- Added serial transcript capture, dry-run hardware safety gates, and config profiles for mock, hardware read-only, and local-only hardware control validation.
- Added connection-state tracking, protocol mismatch counters, replay regression tests, and a localhost-only status endpoint.

## 0.1.0

- Initial local scaffold and Phase 1 protocol documentation.

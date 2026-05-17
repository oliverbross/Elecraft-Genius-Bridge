# Contributing

Elecraft Genius Bridge is station-control infrastructure. Changes should be small, testable, and documented.

## Development Loop

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p egb -- check-config --config config.example.yaml
```

## Protocol Rules

- Do not invent PGXL/TGXL fields.
- Add source evidence, packet captures, or real validation notes before changing protocol behavior.
- Keep protocol parsing separate from shared state and Elecraft serial drivers.
- Mark guessed Elecraft serial commands as unverified until validated.

## Documentation

Update docs with behavior changes. Protocol changes should update:

- `docs/protocol-analysis/pgxl.md`
- `docs/protocol-analysis/tgxl.md`
- `docs/validation/aethersdr-mock-validation.md`
- `docs/open-issues.md`

## Hardware Safety

Use low power, dummy loads, and conservative defaults for hardware validation. Do not run RF-producing tests unattended.


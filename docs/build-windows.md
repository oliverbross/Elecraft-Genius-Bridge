# Build On Windows

Install Rust from `https://rustup.rs`, then open PowerShell in the repository.

```powershell
rustup default stable
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --release -p egb
```

The release binary will be:

```text
target\release\egb.exe
```

The intended deployment target is a single Windows executable plus a YAML config file.


# Development Environment

## Expected Toolchain

Use stable Rust with `rustfmt` and `clippy` installed:

```powershell
rustup default stable
rustup component add rustfmt clippy
```

Expected checks:

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## Current Local Toolchain Finding

In the current Windows shell, `cargo` and `rustc` were not on `PATH`, but a rustup-managed stable toolchain existed at:

```text
C:\Users\Oliver\.rustup\toolchains\stable-x86_64-pc-windows-msvc\bin
```

`cargo test --workspace` runs successfully when using that absolute Cargo path.

The `rustup` executable was not found, and these Cargo subcommands are missing:

```text
cargo fmt
cargo clippy
```

That means `rustfmt` and `clippy` components could not be installed through `rustup` from this shell.

An MSYS2 toolchain also exists under:

```text
C:\JTSDK64-Tools\tools\msys64\mingw64\bin
```

Direct `rustfmt.exe --edition 2021 --check <files>` works from that toolchain. Its `cargo-clippy.exe` exists, but using the MSYS Cargo directly fails because the GNU linker path is incomplete in this shell:

```text
x86_64-w64-mingw32-gcc: fatal error: cannot execute 'ld': CreateProcess: No such file or directory
```

The working local workaround is to prepend the MSYS tool directory to `PATH` while using the MSVC Cargo and pinning `RUSTC`/`RUSTDOC` back to the same MSVC rustup toolchain:

```powershell
$env:PATH = "C:\JTSDK64-Tools\tools\msys64\mingw64\bin;$env:PATH"
$env:RUSTC = "$env:USERPROFILE\.rustup\toolchains\stable-x86_64-pc-windows-msvc\bin\rustc.exe"
$env:RUSTDOC = "$env:USERPROFILE\.rustup\toolchains\stable-x86_64-pc-windows-msvc\bin\rustdoc.exe"
& "$env:USERPROFILE\.rustup\toolchains\stable-x86_64-pc-windows-msvc\bin\cargo.exe" fmt --all -- --check
& "$env:USERPROFILE\.rustup\toolchains\stable-x86_64-pc-windows-msvc\bin\cargo.exe" clippy --workspace --all-targets -- -D warnings
& "$env:USERPROFILE\.rustup\toolchains\stable-x86_64-pc-windows-msvc\bin\cargo.exe" test --workspace
```

That workaround passed locally.

The helper script `scripts\windows\check.ps1` applies this workaround automatically when those paths exist.

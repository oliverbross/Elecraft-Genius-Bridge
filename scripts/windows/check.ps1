$ErrorActionPreference = "Stop"
$RepoRoot = Resolve-Path (Join-Path $PSScriptRoot "..\..")
Set-Location $RepoRoot

$CargoCommand = Get-Command cargo -ErrorAction SilentlyContinue
if ($CargoCommand) {
    $Cargo = $CargoCommand.Source
} else {
    $Cargo = Join-Path $env:USERPROFILE ".rustup\toolchains\stable-x86_64-pc-windows-msvc\bin\cargo.exe"
}

$MsysTools = "C:\JTSDK64-Tools\tools\msys64\mingw64\bin"
if (Test-Path $MsysTools) {
    $env:PATH = "$MsysTools;$env:PATH"
}

$Rustc = Join-Path $env:USERPROFILE ".rustup\toolchains\stable-x86_64-pc-windows-msvc\bin\rustc.exe"
if (Test-Path $Rustc) {
    $env:RUSTC = $Rustc
}
$Rustdoc = Join-Path $env:USERPROFILE ".rustup\toolchains\stable-x86_64-pc-windows-msvc\bin\rustdoc.exe"
if (Test-Path $Rustdoc) {
    $env:RUSTDOC = $Rustdoc
}

& $Cargo fmt --all -- --check
& $Cargo clippy --workspace --all-targets -- -D warnings
& $Cargo test --workspace
& $Cargo run -p egb -- check-config --config config.example.yaml
& $Cargo run -p egb -- check-config --config config.mock.yaml
& $Cargo run -p egb -- check-config --config config.hardware-readonly.yaml
& $Cargo run -p egb -- check-config --config config.hardware-control-local-only.yaml

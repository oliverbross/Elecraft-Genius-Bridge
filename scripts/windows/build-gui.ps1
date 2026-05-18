$ErrorActionPreference = "Stop"
$RepoRoot = Resolve-Path (Join-Path $PSScriptRoot "..\..")
Set-Location $RepoRoot

$Cargo = Get-Command cargo -ErrorAction SilentlyContinue
if ($Cargo) {
    $CargoPath = $Cargo.Source
} else {
    $CargoPath = Join-Path $env:USERPROFILE ".rustup\toolchains\stable-x86_64-pc-windows-msvc\bin\cargo.exe"
}

$Rustc = Join-Path $env:USERPROFILE ".rustup\toolchains\stable-x86_64-pc-windows-msvc\bin\rustc.exe"
if (Test-Path $Rustc) {
    $env:RUSTC = $Rustc
}
$Rustdoc = Join-Path $env:USERPROFILE ".rustup\toolchains\stable-x86_64-pc-windows-msvc\bin\rustdoc.exe"
if (Test-Path $Rustdoc) {
    $env:RUSTDOC = $Rustdoc
}

$env:CARGO_TARGET_DIR = "target-msvc"
& $CargoPath build -p egb -p egb-gui --release
if ($LASTEXITCODE -ne 0) {
    throw "GUI build failed with exit code $LASTEXITCODE"
}

Write-Host "Built:"
Write-Host "  target-msvc\release\egb.exe"
Write-Host "  target-msvc\release\egb-gui.exe"

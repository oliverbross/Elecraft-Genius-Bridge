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

$Dist = Join-Path $RepoRoot "dist\ElecraftGeniusBridge-Windows"
New-Item -ItemType Directory -Force -Path $Dist | Out-Null
Copy-Item "target-msvc\release\egb.exe" $Dist -Force
Copy-Item "target-msvc\release\egb-gui.exe" $Dist -Force
Copy-Item "config.example.yaml" $Dist -Force
Copy-Item "config.mock.yaml" $Dist -Force
Copy-Item "config.hardware-readonly.yaml" $Dist -Force
Copy-Item "config.aethersdr-compat-readonly.yaml" $Dist -Force
Copy-Item "docs\gui-windows.md" $Dist -Force
Copy-Item "docs\controls-safety.md" $Dist -Force -ErrorAction SilentlyContinue
Copy-Item "docs\diagnostics-export.md" $Dist -Force -ErrorAction SilentlyContinue
Copy-Item "docs\smartsdr-tgxl-stability.md" $Dist -Force -ErrorAction SilentlyContinue
Copy-Item "docs\tray-autostart.md" $Dist -Force -ErrorAction SilentlyContinue

Write-Host "Built:"
Write-Host "  target-msvc\release\egb.exe"
Write-Host "  target-msvc\release\egb-gui.exe"
Write-Host "Dist:"
Write-Host "  $Dist"

$ErrorActionPreference = "Stop"
$RepoRoot = Resolve-Path (Join-Path $PSScriptRoot "..\..")
$GuiPath = Join-Path $RepoRoot "target-msvc\release\egb-gui.exe"
if (!(Test-Path $GuiPath)) {
    $GuiPath = Join-Path $RepoRoot "target-msvc\debug\egb-gui.exe"
}
if (!(Test-Path $GuiPath)) {
    throw "egb-gui.exe was not found. Build it first with scripts\windows\build-gui.ps1."
}

$RunKey = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run"
Set-ItemProperty -Path $RunKey -Name "ElecraftGeniusBridgeGui" -Value "`"$GuiPath`""
Write-Host "Installed GUI autostart for current user:"
Write-Host "  $GuiPath"

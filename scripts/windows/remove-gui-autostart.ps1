$ErrorActionPreference = "Stop"
$RunKey = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run"
Remove-ItemProperty -Path $RunKey -Name "ElecraftGeniusBridgeGui" -ErrorAction SilentlyContinue
Write-Host "Removed GUI autostart for current user."

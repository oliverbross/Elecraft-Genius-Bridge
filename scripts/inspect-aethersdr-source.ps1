param(
    [string]$Path = "research\AetherSDR"
)

$ErrorActionPreference = "Stop"

if (!(Test-Path $Path)) {
    throw "AetherSDR source path not found: $Path"
}

$src = Join-Path $Path "src"
if (!(Test-Path $src)) {
    throw "AetherSDR src directory not found: $src"
}

$patterns = @(
    "setTunerVisible",
    "setAmpVisible",
    "presenceChanged",
    "amplifierChanged",
    "model=TunerGeniusXL",
    "PowerGeniusXL",
    "TunerGeniusXL",
    "sub amplifier all",
    "sub atu all",
    "amplifier set",
    "tgxl set",
    "connectToPgxl",
    "connectToTgxl",
    "m_directPresence"
)

foreach ($pattern in $patterns) {
    Write-Host ""
    Write-Host "== $pattern =="
    rg -n --fixed-strings $pattern $src
}

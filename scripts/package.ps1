<#
.SYNOPSIS
  Check, build and package QuickSwitch (plan §10.1).

.DESCRIPTION
  1. cargo fmt --check, cargo clippy -D warnings, cargo test
  2. cargo build --release
  3. dist\QuickSwitch-<version>-portable.zip   (exe + portable marker + README.txt)
  4. dist\QuickSwitch-<version>-setup.exe      (Inno Setup, per-user, no admin)

  Inno Setup 6 is found on PATH, in %LOCALAPPDATA%\Programs\Inno Setup 6, or in
  Program Files. Install it with:  winget install JRSoftware.InnoSetup --scope user

.PARAMETER SkipChecks
  Skip fmt/clippy/test (for a quick repackage).
#>
param([switch]$SkipChecks)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

function Step($name, [scriptblock]$block) {
    Write-Host "==> $name" -ForegroundColor Cyan
    & $block
    if ($LASTEXITCODE -ne 0) { throw "$name failed (exit $LASTEXITCODE)" }
}

$version = (Select-String -Path Cargo.toml -Pattern '^version\s*=\s*"([^"]+)"' | Select-Object -First 1).Matches[0].Groups[1].Value
Write-Host "QuickSwitch $version"

if (-not $SkipChecks) {
    Step 'cargo fmt --check' { cargo fmt --check }
    Step 'cargo clippy' { cargo clippy --all-targets -- -D warnings }
    Step 'cargo test' { cargo test }
}
Step 'cargo build --release' { cargo build --release }

$exe = Join-Path $root 'target\release\QuickSwitch.exe'
$dist = Join-Path $root 'dist'
New-Item -ItemType Directory -Force $dist | Out-Null

# Portable ZIP: the marker file makes the app keep config/log beside the exe.
$stage = Join-Path $dist "QuickSwitch-$version-portable"
if (Test-Path $stage) { Remove-Item -Recurse -Force $stage }
New-Item -ItemType Directory -Force $stage | Out-Null
Copy-Item $exe $stage
# The user README (packaging\README.txt), not the developer README.md.
Copy-Item packaging\README.txt $stage
Set-Content -Path (Join-Path $stage 'QuickSwitch.portable') -Value @(
    'This file turns on portable mode: QuickSwitch keeps config.toml and its log in this folder.'
    'Delete it to use %APPDATA%\QuickSwitch instead.'
)
$zip = Join-Path $dist "QuickSwitch-$version-portable.zip"
if (Test-Path $zip) { Remove-Item -Force $zip }
Compress-Archive -Path (Join-Path $stage '*') -DestinationPath $zip
Remove-Item -Recurse -Force $stage
Write-Host "Portable: $zip"

# Installer.
$iscc = (Get-Command ISCC.exe -ErrorAction SilentlyContinue).Source
if (-not $iscc) {
    $iscc = @(
        "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe",
        "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
        "$env:ProgramFiles\Inno Setup 6\ISCC.exe"
    ) | Where-Object { Test-Path $_ } | Select-Object -First 1
}
if (-not $iscc) { throw 'Inno Setup 6 (ISCC.exe) not found. winget install JRSoftware.InnoSetup --scope user' }
Step 'Inno Setup' { & $iscc /Q "/DAppVersion=$version" "/DSourceExe=$exe" (Join-Path $root 'packaging\QuickSwitch.iss') }
Write-Host "Installer: $(Join-Path $dist "QuickSwitch-$version-setup.exe")"

Get-ChildItem $dist | Format-Table Name, Length

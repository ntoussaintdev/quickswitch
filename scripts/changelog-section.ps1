# Prints the CHANGELOG.md section for one version (without its "## [X.Y.Z]" heading).
# Fails if the section is missing or empty, so a release can't go out without notes.
#
#   ./scripts/changelog-section.ps1 -Version 0.4.0 -OutFile dist/release-notes.md
param(
    [Parameter(Mandatory)][string]$Version,
    [string]$Changelog = (Join-Path $PSScriptRoot '..' 'CHANGELOG.md'),
    [string]$OutFile
)

$ErrorActionPreference = 'Stop'
$Version = $Version.TrimStart('v')

$lines = Get-Content -LiteralPath $Changelog -Encoding utf8
$heading = '^##\s+\[' + [regex]::Escape($Version) + '\]'
$start = -1
for ($i = 0; $i -lt $lines.Count; $i++) {
    if ($lines[$i] -match $heading) { $start = $i + 1; break }
}
if ($start -lt 0) { throw "CHANGELOG.md has no '## [$Version]' section. Add one before releasing." }

$end = $lines.Count
for ($i = $start; $i -lt $lines.Count; $i++) {
    if ($lines[$i] -match '^##\s') { $end = $i; break }
}

$section = ($lines[$start..($end - 1)] -join "`n").Trim()
if (-not $section) { throw "The '## [$Version]' section in CHANGELOG.md is empty." }

if ($OutFile) {
    New-Item -ItemType Directory -Force -Path (Split-Path $OutFile -Parent) | Out-Null
    Set-Content -LiteralPath $OutFile -Value $section -Encoding utf8NoBOM
} else {
    $section
}

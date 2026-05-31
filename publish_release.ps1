#!/usr/bin/env pwsh
# publish_release.ps1 — publish the Windows installer to a GitHub Release.
#
# Windows half of the release. macOS publishes independently from a Mac via
# publish_release.sh, uploading its installers to the SAME GitHub release.
#
# NOTE: this fork has no auto-update server / CDN yet, so this only mirrors the
# NSIS installer to GitHub Releases — no R2/CDN upload, no updater manifest, no
# code-signing step here. When a Jasmine-owned update server exists, restore the
# manifest/upload path (see git history for the previous R2 version).
#
# Prerequisite:  .\build_release.ps1           (NSIS installer)
#
# Usage:
#   .\publish_release.ps1                       # create release + upload installer
#   .\publish_release.ps1 -DryRun               # print what would happen

param([switch]$DryRun)

$ErrorActionPreference = 'Stop'
Set-Location -LiteralPath $PSScriptRoot

function Info($m) { Write-Host "-> $m" }
function Ok($m)   { Write-Host "  [ok] $m" -ForegroundColor Green }
function Warn($m) { Write-Host "  [!] $m"  -ForegroundColor Yellow }
function Die($m)  { Write-Host "  [x] $m"  -ForegroundColor Red; exit 1 }
function Test-NameHasVersionToken($name, $version) {
  $escaped = [regex]::Escape($version)
  return $name -match "(^|[^0-9])$escaped([^0-9]|$)"
}
function Assert-NameHasVersionToken($artifact, $label, $version) {
  if (-not (Test-NameHasVersionToken $artifact.Name $version)) {
    Die "$label version mismatch: expected file name to include $version, got $($artifact.Name)"
  }
}
function Format-Size($path) {
  $bytes = (Get-Item -LiteralPath $path).Length
  if ($bytes -ge 1GB) { return "{0:N1} GB" -f ($bytes / 1GB) }
  if ($bytes -ge 1MB) { return "{0:N1} MB" -f ($bytes / 1MB) }
  if ($bytes -ge 1KB) { return "{0:N1} KB" -f ($bytes / 1KB) }
  return "$bytes B"
}

$Target = 'x86_64-pc-windows-msvc'

if (-not (Get-Command gh -ErrorAction SilentlyContinue)) {
  Die "gh CLI not found - install GitHub CLI and run 'gh auth login'"
}

$ghRepo = "arkyu2077/jasmine"

# -- version (must match across the three manifests) -------------------------
$pkgVer = (node -p "require('./package.json').version" 2>$null)
$confVer = (node -p "require('./src-tauri/tauri.conf.json').version" 2>$null)
$cargoVer = ((Select-String -Path 'src-tauri\Cargo.toml' -Pattern '^version\s*=\s*"(.*)"').Matches[0].Groups[1].Value)
if ($pkgVer -eq $confVer -and $confVer -eq $cargoVer) {
  $Version = $confVer
  Ok "version: $Version (package.json = tauri.conf.json = Cargo.toml)"
} else {
  Die "version mismatch: package.json=$pkgVer tauri.conf.json=$confVer Cargo.toml=$cargoVer"
}
$Notes = "Jasmine v$Version"

# -- locate installer (-setup.exe) -------------------------------------------
$nsisDir = Join-Path $PSScriptRoot "src-tauri\target\$Target\release\bundle\nsis"
if (-not (Test-Path $nsisDir)) { Die "no Windows bundle at $nsisDir - run .\build_release.ps1 first" }

$exe = Get-ChildItem -Path $nsisDir -Filter '*-setup.exe' -ErrorAction SilentlyContinue |
  Where-Object { Test-NameHasVersionToken $_.Name $Version } |
  Sort-Object LastWriteTime -Descending | Select-Object -First 1

if (-not $exe) { Die "no current-version NSIS installer in $nsisDir - run .\build_release.ps1 first" }
Assert-NameHasVersionToken $exe 'installer' $Version
$ghFiles = @($exe.FullName)
Ok "installer: $($exe.Name) ($(Format-Size $exe.FullName))"

Write-Host ""
Info "GitHub release v$Version ($ghRepo) will receive $($ghFiles.Count) asset(s):"
foreach ($f in $ghFiles) { Write-Host "    $(Split-Path $f -Leaf) ($(Format-Size $f))" }
Write-Host ""

if ($DryRun) { Ok "DRY RUN - nothing published."; exit 0 }

$yn = Read-Host "Proceed with GitHub release? [y/N]"
if ($yn -notmatch '^[Yy]$') { Warn "aborted by user"; exit 0 }

# -- GitHub Release -----------------------------------------------------------
# Tag + create (idempotent) and upload the installer. macOS publishes to the
# SAME release from publish_release.sh (whichever runs first creates it; the
# other uploads with --clobber).
$tag = "v$Version"
Info "GitHub release: $ghRepo@$tag"
& git rev-parse -q --verify "refs/tags/$tag" *> $null
if ($LASTEXITCODE -ne 0) { & git tag -a $tag -m "Jasmine $tag"; & git push origin $tag; Ok "tagged $tag" }
else { Ok "tag $tag already exists" }
& gh release view $tag *> $null
if ($LASTEXITCODE -ne 0) { & gh release create $tag --title "Jasmine $tag" --notes "$Notes" --verify-tag; Ok "created GitHub release $tag" }
Info "uploading $($ghFiles.Count) installer(s) - this may take a while"
& gh release upload $tag @ghFiles --clobber
if ($LASTEXITCODE -eq 0) { Ok "uploaded $($ghFiles.Count) installer(s) -> GitHub Release" }
Write-Host ""

Ok "Windows release v$Version published to GitHub Releases"
Warn "REMINDER: bump version in package.json / tauri.conf.json / Cargo.toml BEFORE"
Warn "the next release, or you'll re-publish v$Version."
Warn "macOS publishes separately - run ./publish_release.sh on the Mac."

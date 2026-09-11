<#
.SYNOPSIS
    Builds the portable Windows package: dist/nebula-<version>-windows-x86_64.zip.

.DESCRIPTION
    There is no installer toolchain on this project, so the deliverable is a
    portable folder plus a zip and SHA256SUMS.txt. The script is the only
    definition of what a release contains — CI runs this same file.

    It runs from any working directory (paths come from $PSScriptRoot) and
    downloads nothing itself; cargo uses its own crate cache.

.PARAMETER SkipTests
    Skip `cargo test --workspace`. CI's package job uses this because the test
    job already ran the suite on the same commit.
#>
[CmdletBinding()]
param(
    [switch]$SkipTests
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$repo = Split-Path -Parent $PSScriptRoot

function Invoke-Step {
    param([string]$Name, [scriptblock]$Body)
    Write-Host "==> $Name" -ForegroundColor Cyan
    & $Body
    if ($LASTEXITCODE -ne 0) {
        throw "$Name failed with exit code $LASTEXITCODE"
    }
}

# --- version ---------------------------------------------------------------
# Anchored at the line start so `version = ...` inside [workspace.dependencies]
# cannot match.
$cargoToml = Join-Path $repo 'Cargo.toml'
$versionMatch = Select-String -Path $cargoToml -Pattern '^version\s*=\s*"([^"]+)"' | Select-Object -First 1
if (-not $versionMatch) { throw "no workspace version in $cargoToml" }
$version = $versionMatch.Matches[0].Groups[1].Value
Write-Host "Nebula $version" -ForegroundColor Green

# The notes are written by hand; a version bump must not ship the old ones.
$releaseNotes = Join-Path $repo 'RELEASE-NOTES.md'
if ((Get-Content $releaseNotes -Raw) -notmatch [regex]::Escape($version)) {
    throw "RELEASE-NOTES.md does not mention $version"
}

$name = "nebula-$version-windows-x86_64"
$dist = Join-Path $repo 'dist'
$stage = Join-Path $dist $name
$zip = Join-Path $dist "$name.zip"

# --- build -----------------------------------------------------------------
if ($SkipTests) {
    Write-Host '==> Tests skipped (-SkipTests)' -ForegroundColor Yellow
} else {
    Invoke-Step 'cargo test --workspace' { cargo test --manifest-path $cargoToml --workspace }
}
Invoke-Step 'cargo build --release -p nebula' { cargo build --manifest-path $cargoToml --release -p nebula }

# --- assemble --------------------------------------------------------------
# A stale folder from an earlier version would otherwise survive into the zip.
if (Test-Path $stage) { Remove-Item $stage -Recurse -Force }
New-Item -ItemType Directory -Path $stage -Force | Out-Null

$release = Join-Path $repo 'target/release'
$files = @(
    (Join-Path $release 'nebula.exe'),
    (Join-Path $release 'nebula-gui.exe'),
    (Join-Path $repo 'README.md'),
    (Join-Path $repo 'RELEASE-NOTES.md'),
    (Join-Path $repo 'SYNOLOGY_DEPLOY_GUIDE.md'),
    (Join-Path $repo 'LICENSE'),
    (Join-Path $repo 'THIRD-PARTY-NOTICES.md')
)
foreach ($file in $files) {
    if (-not (Test-Path $file)) { throw "missing package input: $file" }
    Copy-Item $file -Destination $stage
}

# The launcher patches keep their directory: docs/launcher/README.md would
# otherwise overwrite the repository README in the package root.
$launcher = Join-Path $stage 'docs/launcher'
New-Item -ItemType Directory -Path $launcher -Force | Out-Null
Copy-Item (Join-Path $repo 'docs/launcher/README.md') -Destination $launcher
$patches = @(Get-ChildItem -Path (Join-Path $repo 'docs/launcher') -Filter '*.patch')
if ($patches.Count -eq 0) { throw 'no launcher patches found in docs/launcher' }
$patches | Copy-Item -Destination $launcher

# The OFL requires the licence text to travel with the font, and the font is
# embedded in both binaries, so a pointer to the repository is not enough.
$licenses = Join-Path $stage 'licenses'
New-Item -ItemType Directory -Path $licenses -Force | Out-Null
foreach ($file in @(
        (Join-Path $repo 'crates/nebula/assets/fonts/LICENSE-Pretendard.txt'),
        (Join-Path $repo 'crates/nebula/assets/icons/LICENSE-Lucide.txt'))) {
    if (-not (Test-Path $file)) { throw "missing package input: $file" }
    Copy-Item $file -Destination $licenses
}

# --- smoke tests -----------------------------------------------------------
# Run against the packaged copies, not target/release: what ships is what is
# tested.
$exe = Join-Path $stage 'nebula.exe'
$guiExe = Join-Path $stage 'nebula-gui.exe'

Write-Host '==> Smoke: nebula.exe --version' -ForegroundColor Cyan
$reported = (& $exe --version) -join ''
if ($LASTEXITCODE -ne 0) { throw "nebula.exe --version exited $LASTEXITCODE" }
if ($reported -notmatch [regex]::Escape($version)) {
    throw "nebula.exe --version printed '$reported', expected it to name $version"
}
Write-Host "    $reported"

Write-Host '==> Smoke: nebula.exe --help' -ForegroundColor Cyan
& $exe --help | Out-Null
if ($LASTEXITCODE -ne 0) { throw "nebula.exe --help exited $LASTEXITCODE" }

Write-Host '==> Smoke: nebula-gui.exe stays up for 3 s' -ForegroundColor Cyan
$configDir = Join-Path ([System.IO.Path]::GetTempPath()) "nebula-smoke-$([guid]::NewGuid().ToString('N'))"
New-Item -ItemType Directory -Path $configDir -Force | Out-Null
$previousConfigDir = $env:NEBULA_CONFIG_DIR
$env:NEBULA_CONFIG_DIR = $configDir
try {
    $proc = Start-Process -FilePath $guiExe -PassThru
    Start-Sleep -Seconds 3
    if ($proc.HasExited) {
        throw "nebula-gui.exe exited with $($proc.ExitCode) instead of staying up; see $configDir\nebula-gui.log"
    }
    # The windowless binary has no console, so this file is the only place its
    # log can land. An empty config dir here means the routing broke.
    $log = Join-Path $configDir 'nebula-gui.log'
    if (-not (Test-Path $log)) { throw "nebula-gui.exe wrote no log to $log" }

    $proc.Refresh()
    $proc.CloseMainWindow() | Out-Null
    if (-not $proc.WaitForExit(10000)) {
        Write-Warning 'nebula-gui.exe ignored the window close; killing it'
        $proc.Kill()
        if (-not $proc.WaitForExit(10000)) { throw 'nebula-gui.exe could not be stopped' }
    }
    Write-Host "    stayed up, logged to nebula-gui.log, exited $($proc.ExitCode)"
    # Only on success: a failed smoke test leaves nebula-gui.log behind, which
    # is the one place a windowless failure says anything.
    Remove-Item $configDir -Recurse -Force -ErrorAction SilentlyContinue
} finally {
    $env:NEBULA_CONFIG_DIR = $previousConfigDir
}

# --- checksums and zip -----------------------------------------------------
# Relative paths, and SHA256SUMS.txt is written last so it never hashes itself.
$sums = Get-ChildItem -Path $stage -Recurse -File | Sort-Object FullName | ForEach-Object {
    $relative = $_.FullName.Substring($stage.Length + 1).Replace('\', '/')
    "$((Get-FileHash $_.FullName -Algorithm SHA256).Hash.ToLower())  $relative"
}
Set-Content -Path (Join-Path $stage 'SHA256SUMS.txt') -Value $sums -Encoding ascii

if (Test-Path $zip) { Remove-Item $zip -Force }
Compress-Archive -Path $stage -DestinationPath $zip -CompressionLevel Optimal

$zipItem = Get-Item $zip
$zipHash = (Get-FileHash $zip -Algorithm SHA256).Hash.ToLower()
Write-Host ''
Write-Host 'Package ready' -ForegroundColor Green
Write-Host "  path   : $($zipItem.FullName)"
Write-Host "  size   : $([math]::Round($zipItem.Length / 1MB, 2)) MB ($($zipItem.Length) bytes)"
Write-Host "  sha256 : $zipHash"

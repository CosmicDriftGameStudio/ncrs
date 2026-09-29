<#
.SYNOPSIS
    NC-rs installer for Windows.

.EXAMPLE
    irm https://raw.githubusercontent.com/<owner>/ncrs/main/install.ps1 | iex

Installs into %LOCALAPPDATA%\Programs\ncrs, which needs no administrator
rights and is not removed on upgrade. Safe to re-run.
#>

[CmdletBinding()]
param(
    [switch]$Uninstall,
    [string]$BinDir = "$env:LOCALAPPDATA\Programs\ncrs"
)

$ErrorActionPreference = 'Stop'
$Repo = if ($env:NCRS_REPO) { $env:NCRS_REPO } else { 'ncrs/ncrs' }
$BinName = 'ncrs.exe'

function Write-Info($message) { Write-Host "  $message" }

if ($Uninstall) {
    $target = Join-Path $BinDir $BinName
    if (Test-Path $target) {
        Remove-Item $target
        Write-Info "removed $target"
    }
    else {
        Write-Info "nothing to remove at $target"
    }
    $config = Join-Path $env:APPDATA 'ncrs'
    Write-Info "config, if any, is in $config"
    return
}

# --- platform ---------------------------------------------------------------

$arch = $env:PROCESSOR_ARCHITECTURE
switch ($arch) {
    'AMD64' { $cpu = 'x86_64' }
    'ARM64' { $cpu = 'aarch64' }
    'x86'   { $cpu = 'i686' }
    default {
        Write-Error "unsupported architecture: $arch"
        return
    }
}
$target = "$cpu-pc-windows-msvc"

Write-Host 'ncrs installer'
Write-Info "platform   $target"
Write-Info "install to $BinDir"
Write-Host ''

# --- download ---------------------------------------------------------------

$archive = "ncrs-$target.zip"
$base = if ($env:NCRS_BASE_URL) { $env:NCRS_BASE_URL } else { "https://github.com/$Repo/releases/latest/download" }
$tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("ncrs-" + [System.Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force -Path $tmp | Out-Null

try {
    $archivePath = Join-Path $tmp $archive

    try {
        Invoke-WebRequest -Uri "$base/$archive" -OutFile $archivePath -UseBasicParsing
    }
    catch {
        Write-Error "no release found for $target at $base/$archive`n   (this is expected until a v* tag exists)"
        return
    }

    # Verify the checksum when the release publishes one.
    try {
        $sumsPath = Join-Path $tmp 'SHA256SUMS'
        Invoke-WebRequest -Uri "$base/SHA256SUMS" -OutFile $sumsPath -UseBasicParsing
        $expected = (Get-Content $sumsPath | Where-Object { $_ -match " $archive$" } | Select-Object -First 1)
        if ($expected) {
            $actual = (Get-FileHash -Path $archivePath -Algorithm SHA256).Hash.ToLower()
            if ($actual -ne $expected.Split(' ')[0]) {
                Write-Error "checksum mismatch for $archive"
                return
            }
            Write-Info 'checksum ok'
        }
        else {
            Write-Info "no checksum for $archive, skipping"
        }
    }
    catch {
        Write-Info 'no SHA256SUMS published, skipping checksum'
    }

    # --- install ------------------------------------------------------------

    Expand-Archive -Path $archivePath -DestinationPath $tmp -Force
    $src = Get-ChildItem -Path $tmp -Recurse -Filter $BinName | Select-Object -First 1
    if (-not $src) {
        Write-Error "archive did not contain a file called $BinName"
        return
    }

    New-Item -ItemType Directory -Force -Path $BinDir | Out-Null
    Copy-Item $src.FullName (Join-Path $BinDir $BinName) -Force
    Write-Info "installed $(Join-Path $BinDir $BinName)"
}
finally {
    Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
}

$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
if ($userPath -notlike "*$BinDir*") {
    Write-Host ''
    Write-Info "$BinDir is not in your PATH. Add it in"
    Write-Info '  System Properties -> Environment Variables -> Path (user)'
}

Write-Host ''
Write-Host "run it with:  ncrs"

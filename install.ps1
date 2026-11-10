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
$Repo = if ($env:NCRS_REPO) { $env:NCRS_REPO } else { 'CosmicDriftGameStudio/ncrs' }
$BinName = 'ncrs.exe'
# The PDF library sits next to the executable; ncrs looks for it there.
$LibName = 'pdfium.dll'

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
    $libTarget = Join-Path $BinDir $LibName
    if (Test-Path $libTarget) {
        Remove-Item $libTarget
        Write-Info "removed $libTarget"
    }
    $config = Join-Path $env:APPDATA 'ncrs'
    Write-Info "config, if any, is in $config"
    return
}

# --- platform ---------------------------------------------------------------

# PROCESSOR_ARCHITECTURE reports the architecture of the running process, so a
# 32-bit PowerShell on 64-bit Windows would say x86. The OS architecture is
# what decides which build runs.
$arch = $null
try { $arch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString() } catch { }
if (-not $arch) {
    $arch = if ($env:PROCESSOR_ARCHITEW6432) { $env:PROCESSOR_ARCHITEW6432 } else { $env:PROCESSOR_ARCHITECTURE }
}
# Only an x86_64 Windows build is released.
switch ($arch) {
    { $_ -in 'X64', 'AMD64' } { $cpu = 'x86_64' }
    # Windows on ARM runs the x64 build under its emulation, which exists from
    # Windows 11 (build 22000) on.
    { $_ -in 'Arm64', 'ARM64' } {
        if ([System.Environment]::OSVersion.Version.Build -lt 22000) {
            Write-Error "ncrs needs Windows 11 on ARM64 (x64 emulation); this is build $([System.Environment]::OSVersion.Version.Build)"
            return
        }
        $cpu = 'x86_64'
    }
    default {
        Write-Error "unsupported architecture: $arch (ncrs is built for 64-bit Windows only)"
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
        Write-Error "no release found for $target at $base/$archive"
        return
    }

    # Verify the checksum. Required, not optional, for the same reason as in
    # install.sh: a checksum that is skipped when it is inconvenient verifies
    # nothing. The download already succeeded at this point, so a failure to
    # fetch SHA256SUMS means the release is malformed, not that the network
    # is down.
    try {
        $sumsPath = Join-Path $tmp 'SHA256SUMS'
        Invoke-WebRequest -Uri "$base/SHA256SUMS" -OutFile $sumsPath -UseBasicParsing
        $expected = (Get-Content $sumsPath | Where-Object { $_ -match " $archive$" } | Select-Object -First 1)
        if (-not $expected) {
            Write-Error "SHA256SUMS has no entry for $archive. The release is incomplete."
            return
        }
        $actual = (Get-FileHash -Path $archivePath -Algorithm SHA256).Hash.ToLower()
        if ($actual -ne $expected.Split(' ')[0]) {
            Write-Error "checksum mismatch for $archive"
            return
        }
        Write-Info 'checksum ok'
    }
    catch {
        Write-Error "no SHA256SUMS published for the latest release. Refusing to install an unverified binary."
        return
    }

    # --- install ------------------------------------------------------------

    Expand-Archive -Path $archivePath -DestinationPath $tmp -Force
    $src = Get-ChildItem -Path $tmp -Recurse -Filter $BinName | Select-Object -First 1
    if (-not $src) {
        Write-Error "archive did not contain a file called $BinName"
        return
    }

    $libSrc = Get-ChildItem -Path $tmp -Recurse -Filter $LibName | Select-Object -First 1
    if (-not $libSrc) {
        Write-Error "archive did not contain a file called $LibName"
        return
    }

    New-Item -ItemType Directory -Force -Path $BinDir | Out-Null
    Copy-Item $src.FullName (Join-Path $BinDir $BinName) -Force
    Copy-Item $libSrc.FullName (Join-Path $BinDir $LibName) -Force
    $installed = "$(Join-Path $BinDir $BinName) and $(Join-Path $BinDir $LibName)"

    # The local AI model: llama.cpp DLLs next to the executable and its backend
    # modules in backends\. Archives without backends\ (aarch64) link it in.
    $srcDir = $src.DirectoryName
    $backendsSrc = Join-Path $srcDir 'backends'
    if (Test-Path $backendsSrc) {
        foreach ($dll in 'llama.dll', 'ggml.dll', 'ggml-base.dll') {
            $dllSrc = Join-Path $srcDir $dll
            if (-not (Test-Path $dllSrc)) {
                Write-Error "archive has a backends folder but no $dll"
                return
            }
            Copy-Item $dllSrc (Join-Path $BinDir $dll) -Force
        }
        $backendsNew = Join-Path $BinDir 'backends.new'
        $backendsDst = Join-Path $BinDir 'backends'
        Remove-Item -Recurse -Force $backendsNew -ErrorAction SilentlyContinue
        Copy-Item $backendsSrc $backendsNew -Recurse -Force
        Remove-Item -Recurse -Force $backendsDst -ErrorAction SilentlyContinue
        Move-Item $backendsNew $backendsDst
        $installed += ", the llama.cpp libraries and $backendsDst"
    }
    Write-Info "installed $installed"
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

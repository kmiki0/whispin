# Whispin release build
# Mirrors dev.ps1's MSVC env setup, then runs `npm run tauri build`.
# Produces an NSIS installer under src-tauri\target\release\bundle\nsis\.
#
# Usage: pwsh scripts\build.ps1
#        (or: powershell -ExecutionPolicy Bypass -File scripts\build.ps1)

$ErrorActionPreference = "Stop"

$msvcRoot = "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Tools\MSVC"
$sdkRoot = "C:\Program Files (x86)\Windows Kits\10"

$msvcVer = (Get-ChildItem $msvcRoot -Directory | Sort-Object Name -Descending | Select-Object -First 1).Name
$sdkVer = (Get-ChildItem "$sdkRoot\Include" -Directory | Where-Object { $_.Name -match '^10\.' } | Sort-Object Name -Descending | Select-Object -First 1).Name

if (-not $msvcVer) { throw "MSVC tools not found under $msvcRoot" }
if (-not $sdkVer) { throw "Windows SDK not found under $sdkRoot\Include" }

$msvc = Join-Path $msvcRoot $msvcVer

$env:INCLUDE = @(
    "$msvc\include",
    "$sdkRoot\Include\$sdkVer\ucrt",
    "$sdkRoot\Include\$sdkVer\um",
    "$sdkRoot\Include\$sdkVer\shared",
    "$sdkRoot\Include\$sdkVer\winrt",
    "$sdkRoot\Include\$sdkVer\cppwinrt"
) -join ';'

$env:LIB = @(
    "$msvc\lib\x64",
    "$sdkRoot\Lib\$sdkVer\ucrt\x64",
    "$sdkRoot\Lib\$sdkVer\um\x64"
) -join ';'

$env:Path = "$msvc\bin\Hostx64\x64;$sdkRoot\bin\$sdkVer\x64;" `
    + [System.Environment]::GetEnvironmentVariable('Path','Machine') + ';' `
    + [System.Environment]::GetEnvironmentVariable('Path','User')

Set-Location $PSScriptRoot\..

# Stale dev instances will lock target\debug but not target\release; warn if any
# whispin.exe is alive in case it's a previous release run.
$alive = Get-Process whispin -ErrorAction SilentlyContinue
if ($alive) {
    Write-Warning "whispin.exe processes are still running:"
    $alive | Format-Table Id, StartTime
    Write-Warning "If the build fails on a file lock, stop them first (Stop-Process -Id <pid>)."
}

Write-Host ""
Write-Host "=== Building Whispin release (NSIS installer) ===" -ForegroundColor Cyan
Write-Host ""

npm run tauri build

if ($LASTEXITCODE -ne 0) { throw "tauri build failed (exit $LASTEXITCODE)" }

$bundle = Join-Path $PSScriptRoot "..\src-tauri\target\release\bundle\nsis"
$resolved = Resolve-Path $bundle -ErrorAction SilentlyContinue
$bundle = if ($resolved) { $resolved.Path } else { $null }
if ($bundle -and (Test-Path $bundle)) {
    Write-Host ""
    Write-Host "=== Output ===" -ForegroundColor Green
    Get-ChildItem $bundle -File | ForEach-Object {
        $size = "{0:N1} MB" -f ($_.Length / 1MB)
        Write-Host ("  {0,-50} {1}" -f $_.Name, $size)
    }
    Write-Host ""
    Write-Host "Path: $bundle" -ForegroundColor DarkGray
}

# Quick cargo check using the same MSVC env as dev.ps1 / build.ps1.
# Skips bundling so it's a fast compile-only smoke test.

$ErrorActionPreference = "Stop"

$msvcRoot = "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Tools\MSVC"
$sdkRoot = "C:\Program Files (x86)\Windows Kits\10"

$msvcVer = (Get-ChildItem $msvcRoot -Directory | Sort-Object Name -Descending | Select-Object -First 1).Name
$sdkVer = (Get-ChildItem "$sdkRoot\Include" -Directory | Where-Object { $_.Name -match '^10\.' } | Sort-Object Name -Descending | Select-Object -First 1).Name

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

Set-Location $PSScriptRoot\..\src-tauri
cargo check --color=never

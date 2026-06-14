# Whispin dev launcher
# Sets up MSVC env vars (vcvars64 on this machine doesn't populate INCLUDE/LIB
# because the SDK isn't registered in the BuildTools install), then runs the
# Tauri dev server.
#
# Usage: pwsh scripts/dev.ps1
#        (or: powershell -ExecutionPolicy Bypass -File scripts/dev.ps1)

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

if ($env:OPENROUTER_API_KEY) {
    Write-Host "Using OpenRouter (openai/whisper-large-v3-turbo)."
} elseif ($env:OPENAI_API_KEY) {
    Write-Host "Using OpenAI Whisper (whisper-1)."
} elseif ($env:GROQ_API_KEY) {
    Write-Host "Using Groq Whisper (whisper-large-v3-turbo)."
} else {
    Write-Warning "None of OPENROUTER_API_KEY / OPENAI_API_KEY / GROQ_API_KEY is set. Transcription will fail."
}

Set-Location $PSScriptRoot\..
npm run tauri dev

$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
$project = Split-Path $PSScriptRoot -Parent
Push-Location $project
try {
    pnpm run build:ui
    cargo build --locked --release
    $metadata = cargo metadata --locked --no-deps --format-version 1 | ConvertFrom-Json
    $binary = Join-Path $metadata.target_directory 'release/chatgpt-dialog-assistant.exe'
    $out = New-Item -ItemType Directory -Path (Join-Path $project 'outputs') -Force
    Copy-Item -LiteralPath $binary -Destination (Join-Path $out.FullName 'chatgpt对话助手.exe')
} finally {
    Pop-Location
}

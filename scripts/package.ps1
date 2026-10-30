[CmdletBinding()]
param([switch]$SkipBuild)
$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path -Parent $PSScriptRoot
Push-Location $repoRoot
try {
    if (-not $SkipBuild) {
        & cargo build --release --locked
        if ($LASTEXITCODE -ne 0) { throw 'Release build failed.' }
    }
    $versionLine = Get-Content -LiteralPath 'Cargo.toml' | Where-Object { $_ -match '^version = "([^"]+)"' } | Select-Object -First 1
    if ($versionLine -notmatch '^version = "([^"]+)"') { throw 'Package version is missing.' }
    $version = $Matches[1]
    $extensionVersion = (Get-Content -LiteralPath 'extension\manifest.json' -Raw | ConvertFrom-Json).version
    if ($extensionVersion -ne $version) { throw 'App and extension versions must match.' }
    $stage = [IO.Path]::GetFullPath((Join-Path $repoRoot 'build\release-package'))
    $allowedRoot = [IO.Path]::GetFullPath((Join-Path $repoRoot 'build')) + '\'
    if (-not $stage.StartsWith($allowedRoot, [StringComparison]::OrdinalIgnoreCase)) { throw 'Unsafe staging path.' }
    if (Test-Path -LiteralPath $stage) { Remove-Item -LiteralPath $stage -Recurse }
    New-Item -ItemType Directory -Path (Join-Path $stage 'extension\icons') -Force | Out-Null
    New-Item -ItemType Directory -Path (Join-Path $stage 'scripts') -Force | Out-Null
    Copy-Item -LiteralPath 'target\release\ChatGPT-Dikte.exe' -Destination $stage
    Copy-Item -LiteralPath 'scripts\setup.ps1' -Destination (Join-Path $stage 'scripts')
    # Explicit allowlist: never include tokens, settings, logs or developer files.
    foreach ($name in @('background.js','content.js','manifest.json','popup.html','popup.js')) {
        Copy-Item -LiteralPath (Join-Path 'extension' $name) -Destination (Join-Path $stage 'extension')
    }
    foreach ($size in @(16,32,48,128)) {
        Copy-Item -LiteralPath "extension\icons\icon$size.png" -Destination (Join-Path $stage 'extension\icons')
    }
    Copy-Item -LiteralPath 'docs\QUICKSTART.txt' -Destination (Join-Path $stage 'README.txt')
    $utf8 = New-Object Text.UTF8Encoding($false)
    [IO.File]::WriteAllText((Join-Path $stage 'Setup.bat'), "@echo off`r`npowershell.exe -NoProfile -ExecutionPolicy Bypass -File `"%~dp0scripts\setup.ps1`"`r`npause`r`n", $utf8)
    [IO.File]::WriteAllText((Join-Path $stage 'Start.bat'), "@echo off`r`nif not exist `"%~dp0bridge-config.json`" powershell.exe -NoProfile -ExecutionPolicy Bypass -File `"%~dp0scripts\setup.ps1`"`r`nif not exist `"%~dp0bridge-config.json`" exit /b 1`r`nstart `"`" `"%~dp0ChatGPT-Dikte.exe`"`r`n", $utf8)
    New-Item -ItemType Directory -Path 'dist' -Force | Out-Null
    $zipPath = Join-Path $repoRoot "dist\ChatGPT-Dikte-$version-windows-x64.zip"
    Compress-Archive -Path (Join-Path $stage '*') -DestinationPath $zipPath -Force
    $hash = (Get-FileHash -LiteralPath $zipPath -Algorithm SHA256).Hash.ToLowerInvariant()
    [IO.File]::WriteAllText((Join-Path $repoRoot 'dist\SHA256SUMS.txt'), "$hash  $([IO.Path]::GetFileName($zipPath))`n", $utf8)
    Write-Host "Package: $zipPath"
} finally { Pop-Location }

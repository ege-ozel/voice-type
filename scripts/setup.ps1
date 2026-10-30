# Generate per-install bridge credentials without changing existing application settings.
[CmdletBinding()]
param([string]$InstallDir, [string]$ExtensionDir)
$ErrorActionPreference = 'Stop'
$packageRoot = Split-Path -Parent $PSScriptRoot
if (-not $InstallDir) {
    $InstallDir = if (Test-Path -LiteralPath (Join-Path $packageRoot 'ChatGPT-Dikte.exe')) {
        $packageRoot
    } else { Join-Path $packageRoot 'dist' }
}
if (-not $ExtensionDir) { $ExtensionDir = Join-Path $packageRoot 'extension' }
$InstallDir = [IO.Path]::GetFullPath($InstallDir)
$ExtensionDir = [IO.Path]::GetFullPath($ExtensionDir)
New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
$configPath = Join-Path $InstallDir 'bridge-config.json'
$utf8 = New-Object Text.UTF8Encoding($false)
if (Test-Path -LiteralPath $configPath) {
    $config = Get-Content -LiteralPath $configPath -Raw -Encoding UTF8 | ConvertFrom-Json
    if (-not ($config.token -is [string]) -or [string]::IsNullOrWhiteSpace($config.token)) {
        throw 'Existing bridge-config.json is invalid; it was not overwritten.'
    }
    $token = $config.token
} else {
    $bytes = New-Object byte[] 32
    $rng = [Security.Cryptography.RandomNumberGenerator]::Create()
    try { $rng.GetBytes($bytes) } finally { $rng.Dispose() }
    $token = [Convert]::ToBase64String($bytes).TrimEnd('=').Replace('+', '-').Replace('/', '_')
    [IO.File]::WriteAllText($configPath, (@{ token = $token } | ConvertTo-Json), $utf8)
}
if (Test-Path -LiteralPath (Join-Path $ExtensionDir 'manifest.json')) {
    $jsonToken = ConvertTo-Json -InputObject $token -Compress
    [IO.File]::WriteAllText((Join-Path $ExtensionDir 'config.js'), "const BRIDGE_TOKEN = $jsonToken;`n", $utf8)
}
Write-Host "Ready: $InstallDir"
Write-Host 'For browser dictation, load/reload the extension folder in Chrome.'

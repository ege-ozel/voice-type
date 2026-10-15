# Bir dikte köprüsü EXE'sinin açılış süresini, RAM kullanımını ve dosya boyutunu ölçer.
# Kullanım (çalışan köprüyü önce kapatın; ikisi aynı portu kullanır):
#   powershell -ExecutionPolicy Bypass -File rust\measure.ps1 -Exe dist\ChatGPT-Dikte.exe
#   powershell -ExecutionPolicy Bypass -File rust\measure.ps1 -Exe dist\ChatGPT-Dikte-rust.exe
param([Parameter(Mandatory = $true)][string]$Exe, [int]$SettleSeconds = 10)

$Exe = (Resolve-Path $Exe).Path
$name = [IO.Path]::GetFileNameWithoutExtension($Exe)
$config = Join-Path (Split-Path $Exe) 'bridge-config.json'
$token = (Get-Content $config -Raw -Encoding UTF8 | ConvertFrom-Json).token
if (Get-Process -Name $name -ErrorAction SilentlyContinue) { throw "$name zaten çalışıyor; önce kapatın." }

$watch = [Diagnostics.Stopwatch]::StartNew()
$proc = Start-Process -FilePath $Exe -WorkingDirectory (Split-Path $Exe) -PassThru
$ready = $null
while ($watch.Elapsed.TotalSeconds -lt 30) {
    try {
        Invoke-RestMethod -Uri 'http://127.0.0.1:47831/status' -Headers @{Authorization = "Bearer $token"} -TimeoutSec 1 | Out-Null
        $ready = $watch.Elapsed.TotalMilliseconds
        break
    } catch { Start-Sleep -Milliseconds 20 }
}
Start-Sleep -Seconds $SettleSeconds  # arayüz ve iş parçacıkları otursun

# PyInstaller --onefile iki süreç açar (açıcı + asıl); hepsini topla.
$all = @(Get-Process -Name $name -ErrorAction SilentlyContinue)
$working = ($all | Measure-Object WorkingSet64 -Sum).Sum / 1MB
$private = ($all | Measure-Object PrivateMemorySize64 -Sum).Sum / 1MB
$threads = ($all | ForEach-Object { $_.Threads.Count } | Measure-Object -Sum).Sum

[pscustomobject]@{
    Exe              = Split-Path $Exe -Leaf
    'EXE boyutu (MB)' = [math]::Round((Get-Item $Exe).Length / 1MB, 2)
    'Hazır (ms)'      = if ($ready) { [math]::Round($ready) } else { 'yanıt yok' }
    'Süreç sayısı'    = $all.Count
    'Working set (MB)' = [math]::Round($working, 1)
    'Private (MB)'    = [math]::Round($private, 1)
    'İş parçacığı'    = $threads
} | Format-List

$all | Stop-Process -Force

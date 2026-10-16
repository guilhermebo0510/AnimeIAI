# Sobe o VOICEVOX Engine (CPU) como processo local em 127.0.0.1:50021 (S17).
# Uso: powershell -File tools\voicevox\start.ps1 [-Port 50021] [-Threads 4]
# O engine fica em sidecars\voicevox\windows-cpu (baixado do release oficial VOICEVOX/voicevox_engine, nao versionado).
param(
    [int]$Port = 50021,
    [int]$Threads = 4,
    [int]$TimeoutSec = 120
)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$exe = Join-Path $root 'sidecars\voicevox\windows-cpu\run.exe'
$pidFile = Join-Path $PSScriptRoot 'engine.pid'
$log = Join-Path $PSScriptRoot 'engine.log'
if (-not (Test-Path $exe)) { throw "run.exe nao encontrado em $exe (ver sprints\registro\licencas-voz.md para a origem)" }

function Test-Engine { try { (Invoke-WebRequest -UseBasicParsing -TimeoutSec 2 "http://127.0.0.1:$Port/version").StatusCode -eq 200 } catch { $false } }

if (Test-Engine) { Write-Host "VOICEVOX Engine ja esta respondendo em 127.0.0.1:$Port"; exit 0 }

$p = Start-Process -FilePath $exe -ArgumentList @('--host', '127.0.0.1', '--port', $Port, '--cpu_num_threads', $Threads) `
    -WorkingDirectory (Split-Path $exe) -WindowStyle Hidden -PassThru `
    -RedirectStandardOutput $log -RedirectStandardError ($log + '.err')
Set-Content -Path $pidFile -Value $p.Id
$t0 = Get-Date
while (((Get-Date) - $t0).TotalSeconds -lt $TimeoutSec) {
    if ($p.HasExited) { throw "engine encerrou (codigo $($p.ExitCode)); ver $log.err" }
    if (Test-Engine) { Write-Host "VOICEVOX Engine pronto em 127.0.0.1:$Port (pid $($p.Id))"; exit 0 }
    Start-Sleep -Milliseconds 500
}
throw "engine nao respondeu em $TimeoutSec s"

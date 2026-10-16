# Para o VOICEVOX Engine iniciado por start.ps1 (mata a arvore de processos do pid registrado).
$ErrorActionPreference = 'Stop'
$pidFile = Join-Path $PSScriptRoot 'engine.pid'
if (-not (Test-Path $pidFile)) { Write-Host 'nenhum engine.pid; nada a parar'; exit 0 }
$enginePid = [int](Get-Content $pidFile)
if (Get-Process -Id $enginePid -ErrorAction SilentlyContinue) {
    & taskkill.exe /PID $enginePid /T /F | Out-Null
    Write-Host "VOICEVOX Engine (pid $enginePid) parado"
} else { Write-Host "pid $enginePid ja nao existe" }
Remove-Item $pidFile -Force

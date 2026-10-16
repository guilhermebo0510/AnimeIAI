<#
.SYNOPSIS
  Fecha o AnimeIAI Studio (arvore inteira), recompila e reabre com --bridge --no-session; espera o bridge.json e o PING.
.DESCRIPTION
  Licoes: 9 (exe em uso trava o link: matar a arvore antes), 1 (CARGO_INCREMENTAL=0), 8 (nada de --workspace), disco >= 8 GB.
  A janela abre com o layout/preferencias padrao (--no-session nao le nem grava prefs.json).
.EXAMPLE
  tools\relanca.ps1
  tools\relanca.ps1 -NoBuild -Project C:\AnimeIAI\producoes\exemplo-s01.aiai -Size 1600x900
#>
param(
    [switch]$NoBuild,
    [string]$Project = '',
    [string]$Size = '',
    [string]$Workspace = '',
    [int]$TimeoutSec = 60
)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$app = Join-Path $root 'app'
$exe = Join-Path $app 'target\debug\aiai-studio.exe'

# 1) fecha o app e a arvore de processos dele
Get-Process aiai-studio -ErrorAction SilentlyContinue | ForEach-Object { & taskkill /F /T /PID $_.Id 2>&1 | Out-Null }
Start-Sleep -Milliseconds 600
if (Get-Process aiai-studio -ErrorAction SilentlyContinue) { throw 'nao consegui fechar o aiai-studio' }

# 2) descoberta antiga some junto com o app; se sobrou (queda), remove para nao confundir a espera
$disc = if ($env:AIAI_BRIDGE_DISCOVERY) { $env:AIAI_BRIDGE_DISCOVERY } else { Join-Path $env:LOCALAPPDATA 'AnimeIAI\bridge\bridge.json' }
if (Test-Path $disc) { Remove-Item $disc -Force }

# 3) build (com a guarda de disco)
if (-not $NoBuild) {
    & (Join-Path $PSScriptRoot 'disk_guard.ps1') -MinFreeGB 8
    if ($LASTEXITCODE -ne 0) { throw 'espaco livre < 8 GB: rode tools\disk_guard.ps1 -Clean' }
    Push-Location $app
    try {
        $env:CARGO_INCREMENTAL = '0'
        & cargo build -p aiai-studio
        if ($LASTEXITCODE -ne 0) { throw "cargo build falhou ($LASTEXITCODE)" }
    }
    finally { Pop-Location }
}
if (-not (Test-Path $exe)) { throw "nao achei $exe" }

# 4) abre com a bridge
$argsList = @('--bridge', '--no-session')
if ($Project) { $argsList += @('--project', $Project) }
if ($Size) { $argsList += @('--size', $Size) }
if ($Workspace) { $argsList += @('--workspace', $Workspace) }
$p = Start-Process -FilePath $exe -ArgumentList $argsList -WorkingDirectory $app -PassThru
Write-Output "[relanca] pid=$($p.Id) args=$($argsList -join ' ')"

# 5) espera a descoberta e o PING
$deadline = (Get-Date).AddSeconds($TimeoutSec)
$info = $null
while ((Get-Date) -lt $deadline) {
    if ($p.HasExited) { throw "o app saiu com codigo $($p.ExitCode) antes de responder" }
    if (Test-Path $disc) {
        try { $info = Get-Content $disc -Raw | ConvertFrom-Json } catch { $info = $null }
        if ($info -and $info.port) {
            try {
                $c = New-Object System.Net.Sockets.TcpClient('127.0.0.1', [int]$info.port)
                $c.ReceiveTimeout = 5000
                $s = $c.GetStream()
                $req = (@{ id = 'relanca'; action = 'PING'; params = @{}; token = $info.token } | ConvertTo-Json -Compress) + "`n"
                $bytes = [System.Text.Encoding]::UTF8.GetBytes($req)
                $s.Write($bytes, 0, $bytes.Length)
                $r = New-Object System.IO.StreamReader($s)
                $line = $r.ReadLine()
                $c.Close()
                if ($line -and ($line | ConvertFrom-Json).success) {
                    Write-Output "[relanca] PING ok em 127.0.0.1:$($info.port) -> $line"
                    exit 0
                }
            }
            catch { }
        }
    }
    Start-Sleep -Milliseconds 400
}
throw "timeout: a bridge nao respondeu ao PING em ${TimeoutSec}s (descoberta: $disc)"

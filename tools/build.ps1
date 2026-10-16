<#
.SYNOPSIS
  Build do AnimeIAI com a protecao de disco: roda disk_guard, recusa com menos de 8 GB livres e chama `cargo build`.
.EXAMPLE
  tools\build.ps1 -p aiai-studio
  (os argumentos sao repassados ao cargo; nunca use --workspace neste PC)
#>
$ErrorActionPreference = 'Stop'
$here = $PSScriptRoot
$app = Join-Path (Split-Path -Parent $here) 'app'

& (Join-Path $here 'disk_guard.ps1') -MinFreeGB 8
if ($LASTEXITCODE -ne 0) {
    Write-Host 'Build recusado: espaco livre insuficiente (< 8 GB). Rode tools\disk_guard.ps1 -Clean.' -ForegroundColor Red
    exit 1
}

if ($args -contains '--workspace') {
    Write-Host 'Build recusado: --workspace estoura a RAM e o disco deste PC. Use -p <crate>.' -ForegroundColor Red
    exit 1
}

Push-Location $app
try {
    & cargo build @args
    exit $LASTEXITCODE
}
finally {
    Pop-Location
}

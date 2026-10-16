<#
.SYNOPSIS
  Guarda de disco do AnimeIAI: mostra espaco livre de C:, tamanho de app/target e (com -Clean) limpa na ordem das licoes.
.DESCRIPTION
  Ordem de limpeza (sprints/00-LICOES-DE-IMPLEMENTACAO.md, licao 2):
    1) app/target/debug/deps/*.pdb
    2) artefatos duplicados em deps (mantem o mais novo por nome-sem-hash + extensao)
    3) app/target/debug/incremental
  `cargo clean` nunca e chamado aqui (so o usuario decide, no fim).
.PARAMETER Clean
  Executa a limpeza. Sem -Clean so informa.
.PARAMETER MinFreeGB
  Limite de aviso (padrao 8). O script sai com codigo 1 se o livre estiver abaixo.
#>
param(
    [switch]$Clean,
    [double]$MinFreeGB = 8
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$target = Join-Path $root 'app\target'

function Get-FreeGB {
    $d = Get-PSDrive -Name C
    [math]::Round($d.Free / 1GB, 2)
}

function Get-DirSizeGB([string]$path) {
    if (-not (Test-Path $path)) { return 0 }
    $sum = (Get-ChildItem -LiteralPath $path -Recurse -Force -File -ErrorAction SilentlyContinue | Measure-Object -Property Length -Sum).Sum
    if ($null -eq $sum) { return 0 }
    [math]::Round($sum / 1GB, 2)
}

function Show-Status([string]$label) {
    Write-Host ("{0}: C: livre = {1} GB | app/target = {2} GB" -f $label, (Get-FreeGB), (Get-DirSizeGB $target))
}

Show-Status 'Estado'

if ($Clean) {
    $deps = Join-Path $target 'debug\deps'

    # 1) PDBs
    if (Test-Path $deps) {
        $pdbs = Get-ChildItem -LiteralPath $deps -Filter '*.pdb' -File -ErrorAction SilentlyContinue
        $n = ($pdbs | Measure-Object).Count
        $pdbs | Remove-Item -Force -ErrorAction SilentlyContinue
        Write-Host "1) deps/*.pdb removidos: $n"
    }

    # 2) duplicatas em deps: agrupa por (nome sem o hash final) + extensao e mantem o mais novo
    if (Test-Path $deps) {
        $removed = 0
        $files = Get-ChildItem -LiteralPath $deps -File -ErrorAction SilentlyContinue |
            Where-Object { $_.BaseName -match '^(?<name>.+)-(?<hash>[0-9a-f]{16})$' }
        $groups = $files | Group-Object { ($_.BaseName -replace '-[0-9a-f]{16}$', '') + $_.Extension }
        foreach ($g in $groups) {
            if ($g.Count -lt 2) { continue }
            $g.Group | Sort-Object LastWriteTime -Descending | Select-Object -Skip 1 | ForEach-Object {
                Remove-Item -LiteralPath $_.FullName -Force -ErrorAction SilentlyContinue
                $removed++
            }
        }
        Write-Host "2) duplicatas em deps removidas: $removed"
    }

    # 3) incremental
    $inc = Join-Path $target 'debug\incremental'
    if (Test-Path $inc) {
        Remove-Item -LiteralPath $inc -Recurse -Force -ErrorAction SilentlyContinue
        Write-Host '3) debug/incremental removido'
    }
    else {
        Write-Host '3) debug/incremental nao existe'
    }
    Show-Status 'Depois'
}

if ((Get-FreeGB) -lt $MinFreeGB) {
    Write-Host ("ALERTA: menos de {0} GB livres em C:. Nao compile." -f $MinFreeGB) -ForegroundColor Red
    exit 1
}
exit 0

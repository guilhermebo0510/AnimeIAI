<#
.SYNOPSIS
  Caneta sintetica do Windows (Windows Ink, com pressao) para calibrar pinceis com a MESMA entrada no AnimeIAI e em outros apps.
.DESCRIPTION
  Uso:  powershell -NoProfile -ExecutionPolicy Bypass -File tools\harness\pen.ps1 -File tracos.txt [-Interval 8] [-PointerId 0]
  Arquivo (uma operacao por linha, coordenadas em pixels FISICOS da tela):
    X Y P          ponto do traco com pressao P de 0 a 1 (o primeiro ponto depois de 'up' ou do inicio encosta a ponta)
    up             levanta a ponta (fim do traco) e afasta a caneta
    wait MS        pausa
    # ...          comentario
  Cada ponto vira um WM_POINTER de caneta (PT_PEN) com mascara de pressao (0..1024), a ~1000/Interval Hz, como uma mesa.
  So usa CreateSyntheticPointerDevice/InjectSyntheticPointerInput (Windows 10 1809+): nao muda configuracao do sistema.
  Saida: nao-zero se a injecao falhar (mostra o GetLastError).
#>
param(
    [Parameter(Mandatory = $true)][string]$File,
    [int]$Interval = 8,
    [int]$PointerId = 0
)
$ErrorActionPreference = 'Stop'

if (-not ('AiaiPen' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class AiaiPen {
  // POINTER_TYPE_INFO (x64): type @0, uniao @8 (POINTER_TOUCH_INFO = 144 bytes) => 152 bytes.
  // POINTER_PEN_INFO na uniao: POINTER_INFO (96) + penFlags, penMask, pressure, rotation, tiltX, tiltY.
  [StructLayout(LayoutKind.Explicit, Size = 152)]
  public struct PTI {
    [FieldOffset(0)] public uint type;
    [FieldOffset(8)] public uint pointerType;
    [FieldOffset(12)] public uint pointerId;
    [FieldOffset(16)] public uint frameId;
    [FieldOffset(20)] public uint pointerFlags;
    [FieldOffset(24)] public IntPtr sourceDevice;
    [FieldOffset(32)] public IntPtr hwndTarget;
    [FieldOffset(40)] public int px;
    [FieldOffset(44)] public int py;
    [FieldOffset(48)] public int hx;
    [FieldOffset(52)] public int hy;
    [FieldOffset(56)] public int rpx;
    [FieldOffset(60)] public int rpy;
    [FieldOffset(64)] public int rhx;
    [FieldOffset(68)] public int rhy;
    [FieldOffset(72)] public uint dwTime;
    [FieldOffset(76)] public uint historyCount;
    [FieldOffset(80)] public int inputData;
    [FieldOffset(84)] public uint dwKeyStates;
    [FieldOffset(88)] public ulong performanceCount;
    [FieldOffset(96)] public int buttonChangeType;
    [FieldOffset(104)] public uint penFlags;
    [FieldOffset(108)] public uint penMask;
    [FieldOffset(112)] public uint pressure;
    [FieldOffset(116)] public uint rotation;
    [FieldOffset(120)] public int tiltX;
    [FieldOffset(124)] public int tiltY;
  }
  [DllImport("user32.dll", SetLastError = true)] public static extern IntPtr CreateSyntheticPointerDevice(uint type, uint maxCount, uint mode);
  [DllImport("user32.dll", SetLastError = true)] public static extern bool InjectSyntheticPointerInput(IntPtr dev, PTI[] info, uint count);
  [DllImport("user32.dll")] public static extern void DestroySyntheticPointerDevice(IntPtr dev);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();

  public const uint PT_PEN = 3;
  public const uint INRANGE = 0x2, INCONTACT = 0x4, FIRSTBUTTON = 0x10, PRIMARY = 0x2000, DOWN = 0x10000, UPDATE = 0x20000, UP = 0x40000;
  static uint frame = 1;

  public static void Send(IntPtr dev, uint id, int x, int y, double p, uint flags, int change) {
    var a = new PTI[1];
    a[0].type = PT_PEN;
    a[0].pointerType = PT_PEN;
    a[0].pointerId = id;
    a[0].frameId = frame++;
    a[0].pointerFlags = flags;
    a[0].px = x; a[0].py = y;
    a[0].buttonChangeType = change;
    a[0].penMask = 0x1; // PEN_MASK_PRESSURE
    if (p < 0) p = 0; if (p > 1) p = 1;
    a[0].pressure = (uint)Math.Round(p * 1024.0);
    if (!InjectSyntheticPointerInput(dev, a, 1)) throw new Exception("InjectSyntheticPointerInput falhou, erro " + Marshal.GetLastWin32Error());
  }
}
'@
}

[void][AiaiPen]::SetProcessDPIAware()
$dev = [AiaiPen]::CreateSyntheticPointerDevice([AiaiPen]::PT_PEN, 1, 1)
if ($dev -eq [IntPtr]::Zero) { [Console]::Error.WriteLine("[erro] CreateSyntheticPointerDevice falhou"); exit 1 }
$id = [uint32]$PointerId
$R = [AiaiPen]::INRANGE; $C = [AiaiPen]::INCONTACT; $P = [AiaiPen]::PRIMARY
try {
    $down = $false; $lx = 0; $ly = 0; $n = 0; $strokes = 0
    foreach ($raw in [System.IO.File]::ReadAllLines($File)) {
        $l = $raw.Trim(); if (-not $l -or $l.StartsWith('#')) { continue }
        $f = $l -split '\s+'
        if ($f[0] -eq 'up') {
            if ($down) {
                [AiaiPen]::Send($dev, $id, $lx, $ly, 0, ($R -bor $P -bor [AiaiPen]::UP), 2); Start-Sleep -Milliseconds $Interval
                [AiaiPen]::Send($dev, $id, $lx, $ly, 0, ([AiaiPen]::UPDATE -bor $P), 0); Start-Sleep -Milliseconds 60
                $down = $false; $strokes++
            }
            continue
        }
        if ($f[0] -eq 'wait') { Start-Sleep -Milliseconds ([int]$f[1]); continue }
        $x = [int][Math]::Round([double]$f[0]); $y = [int][Math]::Round([double]$f[1]); $pr = [double]$f[2]
        if (-not $down) {
            # aproxima (no ar), depois encosta a ponta
            [AiaiPen]::Send($dev, $id, $x, $y, 0, ($R -bor $P -bor [AiaiPen]::UPDATE), 0); Start-Sleep -Milliseconds 40
            [AiaiPen]::Send($dev, $id, $x, $y, $pr, ($R -bor $C -bor $P -bor [AiaiPen]::DOWN), 1)
            $down = $true
        } else {
            [AiaiPen]::Send($dev, $id, $x, $y, $pr, ($R -bor $C -bor $P -bor [AiaiPen]::UPDATE), 0)
        }
        $lx = $x; $ly = $y; $n++
        Start-Sleep -Milliseconds $Interval
    }
    if ($down) {
        [AiaiPen]::Send($dev, $id, $lx, $ly, 0, ($R -bor $P -bor [AiaiPen]::UP), 2); Start-Sleep -Milliseconds $Interval
        [AiaiPen]::Send($dev, $id, $lx, $ly, 0, ([AiaiPen]::UPDATE -bor $P), 0)
        $strokes++
    }
    Write-Output "[pen] $n pontos, $strokes tracos"
    exit 0
}
catch {
    [Console]::Error.WriteLine("[erro] $($_.Exception.Message)")
    exit 1
}
finally {
    [AiaiPen]::DestroySyntheticPointerDevice($dev)
}

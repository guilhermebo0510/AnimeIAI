<#
.SYNOPSIS
  Arrasto REAL (SendInput) por um caminho de varios pontos, com o botao apertado o tempo todo (um unico traco no canvas).
.DESCRIPTION
  O input.ps1 da harness so tem `drag` reto; este complemento (so evidencia da S03) aperta o botao esquerdo no primeiro ponto,
  desliza por cada trecho em passos curtos (SetCursorPos + MOVE relativo 0,0, licao 18) e solta no ultimo.
  Coordenadas em pixels FISICOS da tela (licao 22). Confere antes que a janela da frente e o AnimeIAI (licao 20).
  Uso: powershell -File polydrag.ps1 -Points "100,100;200,100;200,200;100,200;100,100" [-Step 4] [-Delay 12] [-Button left|middle]
#>
param(
    [Parameter(Mandatory = $true)][string]$Points,
    [int]$Step = 4,
    [int]$Delay = 12,
    [string]$Button = 'left',
    [string]$Keys = ''
)
$ErrorActionPreference = 'Stop'
if (-not ('PdIn' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class PdIn {
  [StructLayout(LayoutKind.Sequential)] public struct MOUSEINPUT { public int dx; public int dy; public uint mouseData; public uint dwFlags; public uint time; public IntPtr dwExtraInfo; }
  [StructLayout(LayoutKind.Sequential)] public struct KEYBDINPUT { public ushort wVk; public ushort wScan; public uint dwFlags; public uint time; public IntPtr dwExtraInfo; }
  [StructLayout(LayoutKind.Explicit)] public struct UNI { [FieldOffset(0)] public MOUSEINPUT mi; [FieldOffset(0)] public KEYBDINPUT ki; }
  [StructLayout(LayoutKind.Sequential)] public struct INPUT { public uint type; public UNI u; }
  [DllImport("user32.dll", SetLastError=true)] public static extern uint SendInput(uint n, INPUT[] p, int cb);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, StringBuilder sb, int max);
  public static int Size() { return Marshal.SizeOf(typeof(INPUT)); }
  public static uint Mouse(int dx, int dy, uint flags) {
    INPUT[] a = new INPUT[1];
    MOUSEINPUT mi = new MOUSEINPUT(); mi.dx = dx; mi.dy = dy; mi.mouseData = 0; mi.dwFlags = flags;
    UNI u = new UNI(); u.mi = mi;
    a[0].type = 0; a[0].u = u;
    return SendInput(1, a, Size());
  }
  public static uint Key(ushort vk, bool up) {
    INPUT[] a = new INPUT[1];
    KEYBDINPUT ki = new KEYBDINPUT(); ki.wVk = vk; ki.dwFlags = up ? 2u : 0u;
    UNI u = new UNI(); u.ki = ki;
    a[0].type = 1; a[0].u = u;
    return SendInput(1, a, Size());
  }
  public static string Title() { StringBuilder sb = new StringBuilder(512); GetWindowText(GetForegroundWindow(), sb, 512); return sb.ToString(); }
}
'@
}
[void][PdIn]::SetProcessDPIAware()
if ([PdIn]::Size() -ne 40) { Write-Error "sizeof(INPUT) = $([PdIn]::Size())"; exit 1 }
function Chk($n, $what) { if ($n -ne 1) { throw "SendInput falhou ($what): retorno $n" } }
$title = [PdIn]::Title()
if (-not $title.StartsWith('AnimeIAI')) { [Console]::Error.WriteLine("[erro] janela da frente '$title' nao e o AnimeIAI"); exit 1 }
$pts = @()
foreach ($p in ($Points -split ';')) { $xy = $p.Trim() -split ','; $pts += , @([int]$xy[0], [int]$xy[1]) }
if ($pts.Count -lt 2) { throw 'precisa de pelo menos 2 pontos' }
$down = if ($Button -eq 'middle') { 32 } else { 2 }
$up = if ($Button -eq 'middle') { 64 } else { 4 }
# teclas segradas durante o arrasto (ex.: -Keys space ou -Keys r)
$vk = @{ 'space' = 32; 'shift' = 16; 'ctrl' = 17; 'alt' = 18 }
$held = @()
foreach ($k in ($Keys -split '\+' | Where-Object { $_ })) {
    $code = if ($vk.ContainsKey($k.ToLower())) { [uint16]$vk[$k.ToLower()] } else { [uint16][int][char]$k.ToUpper() }
    Chk ([PdIn]::Key($code, $false)) "kd $k"; $held += $code; Start-Sleep -Milliseconds 30
}
[void][PdIn]::SetCursorPos($pts[0][0], $pts[0][1]); Start-Sleep -Milliseconds 20
Chk ([PdIn]::Mouse(0, 0, 1)) 'move0'; Start-Sleep -Milliseconds 40
Chk ([PdIn]::Mouse(0, 0, $down)) 'down'; Start-Sleep -Milliseconds 50
$n = 0
for ($i = 1; $i -lt $pts.Count; $i++) {
    $x0 = $pts[$i - 1][0]; $y0 = $pts[$i - 1][1]; $x1 = $pts[$i][0]; $y1 = $pts[$i][1]
    $len = [Math]::Sqrt(($x1 - $x0) * ($x1 - $x0) + ($y1 - $y0) * ($y1 - $y0))
    $steps = [Math]::Max(1, [int][Math]::Ceiling($len / $Step))
    for ($s = 1; $s -le $steps; $s++) {
        $cx = [int][Math]::Round($x0 + ($x1 - $x0) * $s / $steps); $cy = [int][Math]::Round($y0 + ($y1 - $y0) * $s / $steps)
        [void][PdIn]::SetCursorPos($cx, $cy); Chk ([PdIn]::Mouse(0, 0, 1)) 'move'; $n++
        Start-Sleep -Milliseconds $Delay
    }
}
Start-Sleep -Milliseconds 40
Chk ([PdIn]::Mouse(0, 0, $up)) 'up'
[array]::Reverse($held)
foreach ($c in $held) { Chk ([PdIn]::Key($c, $true)) 'ku'; Start-Sleep -Milliseconds 25 }
Start-Sleep -Milliseconds 80
Write-Output "[polydrag] $($pts.Count) pontos, $n movimentos, titulo='$title'"
exit 0

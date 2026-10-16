<#
.SYNOPSIS
  Mouse e teclado REAIS (SendInput) para o AnimeIAI Studio. Coordenadas em pixels FISICOS da tela.
.DESCRIPTION
  Uso:  powershell -NoProfile -ExecutionPolicy Bypass -File tools\harness\input.ps1 -S "foco; max; cap C:\x.png; click 700 400"
  Operacoes (separadas por ';' ou quebra de linha):
    foco                       traz a janela do app (processo aiai-studio) para a frente e CONFERE o foco e o titulo "AnimeIAI..."
    max                        maximiza (SW_MAXIMIZE) e foca
    title                      imprime o titulo da janela e falha se nao comecar com "AnimeIAI"
    rect                       imprime os retangulos fisicos da janela (visivel e area cliente)
    cap <arquivo.png>          captura a janela (bounds visiveis do DWM) para PNG; confere a barra de titulo antes
    wait MS
    move X Y | glide X Y [N]   posiciona o cursor / desliza em N passos
    click X Y | rclick X Y | dclick X Y | mclick X Y
    drag X1 Y1 X2 Y2 [N]       botao esquerdo: aperta em (X1,Y1), desliza N passos, solta em (X2,Y2)
    wheel N                    roda (N positivo = para cima)
    key COMBO                  ex.: key esc | key ctrl+z | key ctrl+shift+s
    type TEXTO                 digita texto Unicode
  Saida: nao-zero se qualquer SendInput falhar (retorno != 1), se a janela nao for achada, se o foco/titulo nao conferir.
  Licoes aplicadas (00-LICOES-DE-IMPLEMENTACAO): 15 DPI (SetProcessDPIAware), 16 sizeof(INPUT)=40, 17 uniao montada em
  variavel propria, 18 SetCursorPos para posicionar e MOVE relativo so para arrastar, 19 BOM UTF-8, 20 foco/titulo antes de
  clicar, 22 coordenadas fisicas (converta de GET_UI_STATE.rect_screen / center_screen, nunca por 1,25).
#>
param(
    [Parameter(Mandatory = $true)][string]$S,
    [string]$Process = 'aiai-studio',
    [string]$TitlePrefix = 'AnimeIAI'
)
$ErrorActionPreference = 'Stop'

if (-not ('AiaiIn' -as [type])) {
    Add-Type -ReferencedAssemblies System.Drawing -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class AiaiIn {
  [StructLayout(LayoutKind.Sequential)] public struct MOUSEINPUT { public int dx; public int dy; public uint mouseData; public uint dwFlags; public uint time; public IntPtr dwExtraInfo; }
  [StructLayout(LayoutKind.Sequential)] public struct KEYBDINPUT { public ushort wVk; public ushort wScan; public uint dwFlags; public uint time; public IntPtr dwExtraInfo; }
  [StructLayout(LayoutKind.Explicit)] public struct UNI { [FieldOffset(0)] public MOUSEINPUT mi; [FieldOffset(0)] public KEYBDINPUT ki; }
  [StructLayout(LayoutKind.Sequential)] public struct INPUT { public uint type; public UNI u; }
  [StructLayout(LayoutKind.Sequential)] public struct PT { public int X; public int Y; }
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
  [DllImport("user32.dll", SetLastError=true)] public static extern uint SendInput(uint n, INPUT[] p, int cb);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern bool GetCursorPos(out PT p);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int c);
  [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr h);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern bool AttachThreadInput(uint a, uint b, bool f);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, StringBuilder sb, int max);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h, ref PT p);
  [DllImport("dwmapi.dll")] public static extern int DwmGetWindowAttribute(IntPtr h, int attr, out RECT r, int size);
  [DllImport("kernel32.dll")] public static extern uint GetCurrentThreadId();
  public static int Size() { return Marshal.SizeOf(typeof(INPUT)); }
  public static uint Mouse(int dx, int dy, uint flags, uint data) {
    INPUT[] a = new INPUT[1];
    MOUSEINPUT mi = new MOUSEINPUT(); mi.dx = dx; mi.dy = dy; mi.mouseData = data; mi.dwFlags = flags;
    UNI u = new UNI(); u.mi = mi;           // licao 17: uniao montada em variavel propria
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
  public static uint Uni(char c, bool up) {
    INPUT[] a = new INPUT[1];
    KEYBDINPUT ki = new KEYBDINPUT(); ki.wVk = 0; ki.wScan = c; ki.dwFlags = 4u | (up ? 2u : 0u);
    UNI u = new UNI(); u.ki = ki;
    a[0].type = 1; a[0].u = u;
    return SendInput(1, a, Size());
  }
  public static string TitleOf(IntPtr h) { StringBuilder sb = new StringBuilder(512); GetWindowText(h, sb, 512); return sb.ToString(); }
}
'@
}
Add-Type -AssemblyName System.Drawing
[void][AiaiIn]::SetProcessDPIAware()
if ([AiaiIn]::Size() -ne 40) { Write-Error "sizeof(INPUT) = $([AiaiIn]::Size()), esperado 40 em x64"; exit 1 }

function Chk($n, $what) { if ($n -ne 1) { throw "SendInput falhou ($what): retorno $n" } }

function Get-AppHandle {
    $proc = Get-Process $Process -ErrorAction SilentlyContinue | Where-Object { $_.MainWindowHandle -ne [IntPtr]::Zero } | Select-Object -First 1
    if (-not $proc) { throw "processo '$Process' nao esta rodando (ou sem janela principal)" }
    return $proc.MainWindowHandle
}

function Assert-Title($h) {
    $t = [AiaiIn]::TitleOf($h)
    if (-not $t.StartsWith($TitlePrefix)) { throw "a janela do processo $Process tem titulo '$t' (esperado comecar com '$TitlePrefix')" }
    return $t
}

function Focus-App {
    $h = Get-AppHandle
    if ([AiaiIn]::IsIconic($h)) { [void][AiaiIn]::ShowWindow($h, 9) }
    $me = [AiaiIn]::GetCurrentThreadId(); $pid2 = 0
    $th = [AiaiIn]::GetWindowThreadProcessId($h, [ref]$pid2)
    [void][AiaiIn]::AttachThreadInput($me, $th, $true)
    [void][AiaiIn]::SetForegroundWindow($h)
    [void][AiaiIn]::AttachThreadInput($me, $th, $false)
    Start-Sleep -Milliseconds 250
    if ([AiaiIn]::GetForegroundWindow() -ne $h) { throw 'foco nao confirmado: outra janela esta na frente' }
    $t = Assert-Title $h
    Write-Output "[foco] ok titulo='$t'"
}

function Get-Visible($h) {
    $r = New-Object AiaiIn+RECT
    $hr = [AiaiIn]::DwmGetWindowAttribute($h, 9, [ref]$r, 16)
    if ($hr -ne 0) { [void][AiaiIn]::GetWindowRect($h, [ref]$r) }
    return $r
}

function Hover([int]$x, [int]$y) {
    [void][AiaiIn]::SetCursorPos($x, $y)             # licao 18: absoluto so por SetCursorPos
    Start-Sleep -Milliseconds 10
    Chk ([AiaiIn]::Mouse(0, 0, 1, 0)) 'move0'         # MOVE relativo 0,0: gera o evento de movimento
    Start-Sleep -Milliseconds 25
}

function Glide([int]$x, [int]$y, [int]$n) {
    $p = New-Object AiaiIn+PT; [void][AiaiIn]::GetCursorPos([ref]$p)
    for ($i = 1; $i -le $n; $i++) {
        $cx = [int][Math]::Round($p.X + ($x - $p.X) * $i / $n); $cy = [int][Math]::Round($p.Y + ($y - $p.Y) * $i / $n)
        [void][AiaiIn]::SetCursorPos($cx, $cy); Chk ([AiaiIn]::Mouse(0, 0, 1, 0)) 'glide'; Start-Sleep -Milliseconds 16
    }
}

$VK = @{ 'esc'=27; 'enter'=13; 'tab'=9; 'space'=32; 'back'=8; 'del'=46; 'delete'=46; 'ins'=45; 'home'=36; 'end'=35; 'pgup'=33; 'pgdn'=34;
  'left'=37; 'up'=38; 'right'=39; 'down'=40; 'ctrl'=17; 'shift'=16; 'alt'=18; 'win'=91;
  'f1'=112; 'f2'=113; 'f3'=114; 'f4'=115; 'f5'=116; 'f6'=117; 'f7'=118; 'f8'=119; 'f9'=120; 'f10'=121; 'f11'=122; 'f12'=123;
  'minus'=189; 'plus'=187; 'comma'=188; 'period'=190 }
function VkOf([string]$name) {
    $k = $name.ToLower()
    if ($VK.ContainsKey($k)) { return [uint16]$VK[$k] }
    if ($k.Length -eq 1) { return [uint16][int][char]$k.ToUpper() }
    throw "tecla desconhecida: $name"
}

try {
    $cmds = $S -split "[;`n]"
    $n = 0
    foreach ($raw in $cmds) {
        $c = $raw.Trim(); if (-not $c) { continue }
        $parts = $c -split '\s+'
        $op = $parts[0].ToLower()
        $n++
        switch ($op) {
            'foco' { Focus-App }
            'max' { $h = Get-AppHandle; [void][AiaiIn]::ShowWindow($h, 3); Start-Sleep -Milliseconds 900; Focus-App }
            'title' { $h = Get-AppHandle; Write-Output "[title] '$(Assert-Title $h)'" }
            'rect' {
                $h = Get-AppHandle; $v = Get-Visible $h
                $cr = New-Object AiaiIn+RECT; [void][AiaiIn]::GetClientRect($h, [ref]$cr)
                $o = New-Object AiaiIn+PT; [void][AiaiIn]::ClientToScreen($h, [ref]$o)
                Write-Output ("[rect] visivel=({0},{1},{2},{3}) cliente_origem=({4},{5}) cliente_tamanho=({6},{7})" -f $v.Left, $v.Top, ($v.Right - $v.Left), ($v.Bottom - $v.Top), $o.X, $o.Y, $cr.Right, $cr.Bottom)
            }
            'cap' {
                if ($parts.Count -lt 2) { throw 'cap exige o arquivo .png' }
                $out = $c.Substring(3).Trim()
                $h = Get-AppHandle; $t = Assert-Title $h
                $v = Get-Visible $h; $w = $v.Right - $v.Left; $hh = $v.Bottom - $v.Top
                $bmp = New-Object System.Drawing.Bitmap($w, $hh)
                $g = [System.Drawing.Graphics]::FromImage($bmp)
                $g.CopyFromScreen($v.Left, $v.Top, 0, 0, (New-Object System.Drawing.Size($w, $hh)))
                $g.Dispose()
                $dir = Split-Path -Parent $out
                if ($dir -and -not (Test-Path $dir)) { New-Item -ItemType Directory -Force $dir | Out-Null }
                $bmp.Save($out, [System.Drawing.Imaging.ImageFormat]::Png); $bmp.Dispose()
                Write-Output "[cap] $out ${w}x${hh} titulo='$t' origem=($($v.Left),$($v.Top))"
            }
            'wait' { Start-Sleep -Milliseconds ([int]$parts[1]) }
            'move' { Hover ([int]$parts[1]) ([int]$parts[2]) }
            'glide' { $steps = if ($parts.Count -gt 3) { [int]$parts[3] } else { 15 }; Glide ([int]$parts[1]) ([int]$parts[2]) $steps }
            'click' { Hover ([int]$parts[1]) ([int]$parts[2]); Chk ([AiaiIn]::Mouse(0,0,2,0)) 'ld'; Start-Sleep -Milliseconds 50; Chk ([AiaiIn]::Mouse(0,0,4,0)) 'lu'; Start-Sleep -Milliseconds 80 }
            'dclick' { Hover ([int]$parts[1]) ([int]$parts[2]); 1..2 | ForEach-Object { Chk ([AiaiIn]::Mouse(0,0,2,0)) 'ld'; Start-Sleep -Milliseconds 30; Chk ([AiaiIn]::Mouse(0,0,4,0)) 'lu'; Start-Sleep -Milliseconds 60 }; Start-Sleep -Milliseconds 60 }
            'rclick' { Hover ([int]$parts[1]) ([int]$parts[2]); Chk ([AiaiIn]::Mouse(0,0,8,0)) 'rd'; Start-Sleep -Milliseconds 50; Chk ([AiaiIn]::Mouse(0,0,16,0)) 'ru'; Start-Sleep -Milliseconds 80 }
            'mclick' { Hover ([int]$parts[1]) ([int]$parts[2]); Chk ([AiaiIn]::Mouse(0,0,32,0)) 'md'; Start-Sleep -Milliseconds 50; Chk ([AiaiIn]::Mouse(0,0,64,0)) 'mu'; Start-Sleep -Milliseconds 80 }
            'drag' {
                $steps = if ($parts.Count -gt 5) { [int]$parts[5] } else { 20 }
                Hover ([int]$parts[1]) ([int]$parts[2]); Chk ([AiaiIn]::Mouse(0,0,2,0)) 'ld'; Start-Sleep -Milliseconds 60
                Glide ([int]$parts[3]) ([int]$parts[4]) $steps; Start-Sleep -Milliseconds 60
                Chk ([AiaiIn]::Mouse(0,0,4,0)) 'lu'; Start-Sleep -Milliseconds 80
            }
            'wheel' { $d = 120 * [int]$parts[1]; $data = [uint32](([int64]$d) -band 0xFFFFFFFFL); Chk ([AiaiIn]::Mouse(0,0,0x800,$data)) 'wheel'; Start-Sleep -Milliseconds 50 }
            'key' {
                $keys = $parts[1] -split '\+'
                foreach ($k in $keys) { Chk ([AiaiIn]::Key((VkOf $k), $false)) "kd $k"; Start-Sleep -Milliseconds 25 }
                [array]::Reverse($keys)
                foreach ($k in $keys) { Chk ([AiaiIn]::Key((VkOf $k), $true)) "ku $k"; Start-Sleep -Milliseconds 25 }
                Start-Sleep -Milliseconds 80
            }
            'type' {
                $t = $c.Substring(4).TrimStart()
                foreach ($ch in $t.ToCharArray()) { Chk ([AiaiIn]::Uni($ch, $false)) 'ud'; Chk ([AiaiIn]::Uni($ch, $true)) 'uu'; Start-Sleep -Milliseconds 15 }
            }
            default { throw "operacao desconhecida: $op" }
        }
    }
    Write-Output "[ok] $n operacoes"
    exit 0
}
catch {
    [Console]::Error.WriteLine("[erro] $($_.Exception.Message)")
    exit 1
}

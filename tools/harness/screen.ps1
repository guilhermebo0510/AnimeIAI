<#
.SYNOPSIS
  Captura de tela DPI-aware (tela inteira ou janela por titulo) para PNG.
.DESCRIPTION
  Chama SetProcessDPIAware antes de tudo (licao 15: sem isso, em 125% as coordenadas saem 25% fora).
  Com -Title captura so a janela cujo titulo e exatamente o texto (ou, na falta, contem o texto) (traz para a frente antes); sem -Title, a tela inteira.
.EXAMPLE
  tools\harness\screen.ps1 -Title "AnimeIAI" -Out C:\AnimeIAI\sprints\registro\evidencias\S00\janela.png
#>
param(
    [string]$Title = '',
    [Parameter(Mandatory = $true)][string]$Out
)

$ErrorActionPreference = 'Stop'

Add-Type -AssemblyName System.Drawing
Add-Type @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class Win32Screen {
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
    public delegate bool EnumWindowsProc(IntPtr hWnd, IntPtr lParam);
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumWindowsProc cb, IntPtr lParam);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, StringBuilder sb, int max);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int cmd);
    [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr h);
    [DllImport("user32.dll")] public static extern int GetSystemMetrics(int i);
    // Titulo exato primeiro (o Claude Desktop tem "AnimeIAI" no titulo: licao 20), depois "contem".
    public static IntPtr FindByTitle(string part) {
        IntPtr exact = IntPtr.Zero, partial = IntPtr.Zero;
        EnumWindows((h, l) => {
            if (!IsWindowVisible(h)) return true;
            var sb = new StringBuilder(512);
            GetWindowText(h, sb, 512);
            if (sb.Length == 0) return true;
            string t = sb.ToString();
            if (string.Equals(t, part, StringComparison.OrdinalIgnoreCase)) { exact = h; return false; }
            if (partial == IntPtr.Zero && t.IndexOf(part, StringComparison.OrdinalIgnoreCase) >= 0) partial = h;
            return true;
        }, IntPtr.Zero);
        return exact != IntPtr.Zero ? exact : partial;
    }
}
'@

[void][Win32Screen]::SetProcessDPIAware()

if ($Title -ne '') {
    $h = [Win32Screen]::FindByTitle($Title)
    if ($h -eq [IntPtr]::Zero) { throw "Janela com titulo contendo '$Title' nao encontrada." }
    if ([Win32Screen]::IsIconic($h)) { [void][Win32Screen]::ShowWindow($h, 9) }
    [void][Win32Screen]::SetForegroundWindow($h)
    Start-Sleep -Milliseconds 400
    $r = New-Object Win32Screen+RECT
    [void][Win32Screen]::GetWindowRect($h, [ref]$r)
    $x = $r.Left; $y = $r.Top; $w = $r.Right - $r.Left; $hgt = $r.Bottom - $r.Top
}
else {
    $x = 0; $y = 0
    $w = [Win32Screen]::GetSystemMetrics(0)
    $hgt = [Win32Screen]::GetSystemMetrics(1)
}

if ($w -le 0 -or $hgt -le 0) { throw "Area de captura invalida: ${w}x${hgt}" }

$dir = Split-Path -Parent $Out
if ($dir -and -not (Test-Path $dir)) { New-Item -ItemType Directory -Force -Path $dir | Out-Null }

$bmp = New-Object System.Drawing.Bitmap $w, $hgt
$g = [System.Drawing.Graphics]::FromImage($bmp)
try {
    $g.CopyFromScreen($x, $y, 0, 0, $bmp.Size)
    $bmp.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png)
}
finally {
    $g.Dispose()
    $bmp.Dispose()
}
Write-Host ("captura salva: {0} ({1}x{2})" -f $Out, $w, $hgt)

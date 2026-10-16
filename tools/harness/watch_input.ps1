<#
.SYNOPSIS
  Vigia de mouse (hook WH_MOUSE_LL): conta eventos injetados (SendInput, injected=1) e FISICOS (injected=0).
.DESCRIPTION
  Roda em paralelo ao teste de mouse real (licao 21). Se aparecer QUALQUER evento fisico (mao do usuario no mouse) a corrida
  esta contaminada: o relatorio marca contaminated=true e o script sai com codigo 3 - descarte a corrida.
  Uso (em segundo plano):
    powershell -NoProfile -ExecutionPolicy Bypass -File tools\harness\watch_input.ps1 -Out C:\x\watch.json -StopFile C:\x\watch.stop -MaxSeconds 120
  e, quando o teste acabar:  New-Item C:\x\watch.stop   (o vigia grava o relatorio e sai).
  Relatorio JSON: {physical, injected, moves_physical, clicks_physical, wheel_physical, first_physical_ms, contaminated}.
#>
param(
    [Parameter(Mandatory = $true)][string]$Out,
    [Parameter(Mandatory = $true)][string]$StopFile,
    [int]$MaxSeconds = 120
)
$ErrorActionPreference = 'Stop'

if (-not ('AiaiWatch' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.Diagnostics;
using System.Runtime.InteropServices;
public static class AiaiWatch {
  [StructLayout(LayoutKind.Sequential)] public struct PT { public int x; public int y; }
  [StructLayout(LayoutKind.Sequential)] public struct MSLLHOOKSTRUCT { public PT pt; public uint mouseData; public uint flags; public uint time; public IntPtr dwExtraInfo; }
  [StructLayout(LayoutKind.Sequential)] public struct MSG { public IntPtr hwnd; public uint message; public IntPtr wParam; public IntPtr lParam; public uint time; public PT pt; }
  delegate IntPtr Proc(int code, IntPtr wParam, IntPtr lParam);
  [DllImport("user32.dll", SetLastError=true)] static extern IntPtr SetWindowsHookEx(int id, Proc p, IntPtr mod, uint thread);
  [DllImport("user32.dll")] static extern bool UnhookWindowsHookEx(IntPtr h);
  [DllImport("user32.dll")] static extern IntPtr CallNextHookEx(IntPtr h, int code, IntPtr w, IntPtr l);
  [DllImport("user32.dll")] static extern bool PeekMessage(out MSG m, IntPtr h, uint min, uint max, uint remove);
  [DllImport("kernel32.dll")] static extern IntPtr GetModuleHandle(string name);
  public static long Physical, Injected, MovesPhysical, ClicksPhysical, WheelPhysical, FirstPhysicalMs = -1;
  static Proc keep; static IntPtr hook; static Stopwatch sw = Stopwatch.StartNew();
  static IntPtr Cb(int code, IntPtr w, IntPtr l) {
    if (code >= 0) {
      MSLLHOOKSTRUCT s = (MSLLHOOKSTRUCT)Marshal.PtrToStructure(l, typeof(MSLLHOOKSTRUCT));
      bool injected = (s.flags & 0x1) != 0 || (s.flags & 0x2) != 0;   // LLMHF_INJECTED | LLMHF_LOWER_IL_INJECTED
      int msg = w.ToInt32();
      if (injected) Injected++;
      else {
        Physical++;
        if (FirstPhysicalMs < 0) FirstPhysicalMs = sw.ElapsedMilliseconds;
        if (msg == 0x0200) MovesPhysical++;
        else if (msg == 0x020A || msg == 0x020E) WheelPhysical++;
        else ClicksPhysical++;
      }
    }
    return CallNextHookEx(hook, code, w, l);
  }
  public static void Start() {
    keep = Cb;
    hook = SetWindowsHookEx(14, keep, GetModuleHandle(null), 0);
    if (hook == IntPtr.Zero) throw new Exception("SetWindowsHookEx(WH_MOUSE_LL) falhou: " + Marshal.GetLastWin32Error());
  }
  public static void Pump() { MSG m; while (PeekMessage(out m, IntPtr.Zero, 0, 0, 1)) { } }
  public static void Stop() { if (hook != IntPtr.Zero) UnhookWindowsHookEx(hook); hook = IntPtr.Zero; }
}
'@
}

if (Test-Path $StopFile) { Remove-Item $StopFile -Force }
[AiaiWatch]::Start()
Write-Output "[watch] hook ativo; pare criando $StopFile (limite ${MaxSeconds}s)"
$t0 = Get-Date
try {
    while (-not (Test-Path $StopFile) -and ((Get-Date) - $t0).TotalSeconds -lt $MaxSeconds) {
        [AiaiWatch]::Pump()          # o hook de baixo nivel so dispara com a fila de mensagens da thread sendo bombeada
        Start-Sleep -Milliseconds 5
    }
}
finally {
    [AiaiWatch]::Pump()
    [AiaiWatch]::Stop()
}
$report = [ordered]@{
    physical         = [AiaiWatch]::Physical
    injected         = [AiaiWatch]::Injected
    moves_physical   = [AiaiWatch]::MovesPhysical
    clicks_physical  = [AiaiWatch]::ClicksPhysical
    wheel_physical   = [AiaiWatch]::WheelPhysical
    first_physical_ms = [AiaiWatch]::FirstPhysicalMs
    seconds          = [Math]::Round(((Get-Date) - $t0).TotalSeconds, 1)
    contaminated     = ([AiaiWatch]::Physical -gt 0)
}
$dir = Split-Path -Parent $Out
if ($dir -and -not (Test-Path $dir)) { New-Item -ItemType Directory -Force $dir | Out-Null }
($report | ConvertTo-Json) | Set-Content -Path $Out -Encoding UTF8
Write-Output ($report | ConvertTo-Json -Compress)
if ($report.contaminated) { Write-Output '[watch] CONTAMINADA: houve evento fisico de mouse; descarte a corrida'; exit 3 }
exit 0

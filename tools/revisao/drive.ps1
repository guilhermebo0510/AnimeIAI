param(
    [string]$Proc,
    [string]$Action,          # shot | click | send | focus
    [int]$X = 0,
    [int]$Y = 0,
    [string]$TextFile = "",
    [string]$Out = ""
)
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class W {
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int c);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr h);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(int f, int dx, int dy, int d, int e);
  [DllImport("user32.dll")] public static extern void keybd_event(byte k, byte s, int f, int e);
}
"@
function Focus($name) {
    $p = Get-Process $name -ErrorAction Stop | Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1
    if ([W]::IsIconic($p.MainWindowHandle)) { [W]::ShowWindow($p.MainWindowHandle, 9) | Out-Null }
    [W]::ShowWindow($p.MainWindowHandle, 3) | Out-Null
    # ALT solto ajuda o SetForegroundWindow a funcionar vindo de outro processo
    [W]::keybd_event(0x12, 0, 0, 0); [W]::keybd_event(0x12, 0, 2, 0)
    [W]::SetForegroundWindow($p.MainWindowHandle) | Out-Null
    Start-Sleep -Milliseconds 600
    if ([W]::GetForegroundWindow() -ne $p.MainWindowHandle) { [W]::keybd_event(0x12, 0, 0, 0); [W]::keybd_event(0x12, 0, 2, 0); [W]::SetForegroundWindow($p.MainWindowHandle) | Out-Null; Start-Sleep -Milliseconds 600 }
    if ([W]::GetForegroundWindow() -ne $p.MainWindowHandle) { Write-Output "SEM_FOCO"; exit 3 }
}
function Shot($path) {
    $b = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
    $bmp = New-Object System.Drawing.Bitmap $b.Width, $b.Height
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.CopyFromScreen($b.Location, [System.Drawing.Point]::Empty, $b.Size)
    $bmp.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
    $g.Dispose(); $bmp.Dispose()
}
function Click($x, $y) {
    [W]::SetCursorPos($x, $y); Start-Sleep -Milliseconds 120
    [W]::mouse_event(2, 0, 0, 0, 0); Start-Sleep -Milliseconds 60; [W]::mouse_event(4, 0, 0, 0, 0)
    Start-Sleep -Milliseconds 300
}
if ($Proc) { Focus $Proc }
switch ($Action) {
    "shot" { Shot $Out }
    "click" { Click $X $Y }
    "send" {
        Click $X $Y
        $t = Get-Content -Raw -Encoding UTF8 $TextFile
        Set-Clipboard -Value $t
        Start-Sleep -Milliseconds 300
        [System.Windows.Forms.SendKeys]::SendWait("^v")
        Start-Sleep -Milliseconds 900
        [System.Windows.Forms.SendKeys]::SendWait("{ENTER}")
        Start-Sleep -Milliseconds 800
        if ($Out) { Shot $Out }
    }
    "focus" { }
}

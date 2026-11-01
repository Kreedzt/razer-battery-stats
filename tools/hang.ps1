param()
Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
using System.Text;
public class HUNG {
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindow(string c, string t);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindowEx(IntPtr p, IntPtr a, string c, string t);
  [DllImport("user32.dll")] public static extern bool IsHungAppWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern IntPtr GetWindowDC(IntPtr h);
  [DllImport("user32.dll")] public static extern int ReleaseDC(IntPtr h, IntPtr dc);
  [DllImport("gdi32.dll")] public static extern uint GetPixel(IntPtr dc, int x, int y);
  [DllImport("user32.dll")] public static extern int GetWindowThreadProcessId(IntPtr h, out int pid);
  public struct RECT { public int L, T, R, B; }
}
"@
[HUNG]::SetProcessDPIAware() | Out-Null
$tray = [HUNG]::FindWindow('Shell_TrayWnd', $null)
foreach ($cls in @('DynamicContent2','TrayNotifyWnd')) {
  $h = [HUNG]::FindWindowEx($tray, [IntPtr]::Zero, $cls, $null)
  if ($h -eq [IntPtr]::Zero) { "$cls absent"; continue }
  $p = 0
  [HUNG]::GetWindowThreadProcessId($h, [ref]$p) | Out-Null
  $r = New-Object HUNG+RECT
  [HUNG]::GetWindowRect($h, [ref]$r) | Out-Null
  $sw = [Diagnostics.Stopwatch]::StartNew()
  $dc = [HUNG]::GetWindowDC($h)
  $tDc = $sw.ElapsedMilliseconds
  $sampled = 0
  for ($x = 2; $x -lt ($r.R - $r.L) - 2; $x += 4) {
    for ($y = 2; $y -lt ($r.B - $r.T) - 2; $y += 3) {
      [void][HUNG]::GetPixel($dc, $x, $y); $sampled++
    }
  }
  [HUNG]::ReleaseDC($h, $dc) | Out-Null
  $sw.Stop()
  "{0,-18} pid={1,-6} hung={2,-5} rect={3}..{4} samples={5} GetWindowDC={6}ms total={7}ms" -f `
    $cls, $p, [HUNG]::IsHungAppWindow($h), $r.L, $r.R, $sampled, $tDc, $sw.ElapsedMilliseconds
}
$panel = (Get-Process razer-battery-taskbar -ErrorAction SilentlyContinue | Select-Object -First 1)
if ($panel) {
  $ph = (Get-Process -Id $panel.Id).MainWindowHandle
  "panel pid=$($panel.Id) responding=$($panel.Responding)"
}

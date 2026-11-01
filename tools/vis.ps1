param()
Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
using System.Text;
public class VIS {
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindow(string c, string t);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindowEx(IntPtr p, IntPtr a, string c, string t);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr h);
  [DllImport("user32.dll")] public static extern int GetWindowLong(IntPtr h, int i);
  public struct RECT { public int L, T, R, B; }
}
"@
[VIS]::SetProcessDPIAware() | Out-Null
$tray = [VIS]::FindWindow('Shell_TrayWnd', $null)
foreach ($cls in @('DynamicContent2','DynamicContent1','TrayNotifyWnd')) {
  $h = [VIS]::FindWindowEx($tray, [IntPtr]::Zero, $cls, $null)
  if ($h -eq [IntPtr]::Zero) { "$cls : absent"; continue }
  $r = New-Object VIS+RECT
  [VIS]::GetWindowRect($h, [ref]$r) | Out-Null
  $style = [VIS]::GetWindowLong($h, -16)
  "{0,-18} hwnd={1,-9} visible={2,-5} rect={3}..{4} w={5} style=0x{6}" -f `
    $cls, $h, [VIS]::IsWindowVisible($h), $r.L, $r.R, ($r.R-$r.L), $style.ToString('X8')
}

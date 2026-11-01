param()
Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
using System.Text;
public class SC {
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindow(string c, string t);
  [DllImport("user32.dll")] public static extern bool EnumChildWindows(IntPtr p, IntPtr cb, IntPtr l);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern IntPtr GetWindow(IntPtr h, uint c);
  public struct RECT { public int L, T, R, B; }
  public delegate bool Cb(IntPtr h, IntPtr l);
}
"@
[SC]::SetProcessDPIAware() | Out-Null
$rows = New-Object System.Collections.ArrayList
$want = @('RazerBatteryPanel','MSTaskSwWClass','DynamicContent2','TrayNotifyWnd','ReBarWindow32','Shell_TrayWnd','SysPager','TrayClockWClass')
$cb = [SC+Cb]{ param($h, $l)
  $sb = New-Object System.Text.StringBuilder 256
  [SC]::GetClassName($h, $sb, 256) | Out-Null
  $c = $sb.ToString()
  if ($want -contains $c) {
    $r = New-Object SC+RECT
    [SC]::GetWindowRect($h, [ref]$r) | Out-Null
    if ($r.R -gt $r.L) {
      [void]$rows.Add(("{0,-22} x={1,5}..{2,-5} w={3,4}" -f $c, $r.L, $r.R, ($r.R - $r.L)))
    }
  }
  return $true
}
$gc = [System.Runtime.InteropServices.Marshal]::GetFunctionPointerForDelegate($cb)
$tray = [SC]::FindWindow('Shell_TrayWnd', $null)
[SC]::EnumChildWindows($tray, $gc, [IntPtr]::Zero) | Out-Null
$rows | Sort-Object { [int]($_ -split 'x=')[1].Split('.')[0] }

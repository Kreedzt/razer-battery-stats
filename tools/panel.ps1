param()
Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
using System.Text;
public class PW5 {
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool EnumWindows(IntPtr cb, IntPtr l);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern int GetWindowThreadProcessId(IntPtr h, out int pid);
  public struct RECT { public int L, T, R, B; }
  public delegate bool Cb(IntPtr h, IntPtr l);
}
"@
[PW5]::SetProcessDPIAware() | Out-Null
$rows = New-Object System.Collections.ArrayList
$cb = [PW5+Cb]{ param($h, $l)
  $sb = New-Object System.Text.StringBuilder 256
  [PW5]::GetClassName($h, $sb, 256) | Out-Null
  if ($sb.ToString() -eq 'RazerBatteryPanel') {
    $r = New-Object PW5+RECT
    [PW5]::GetWindowRect($h, [ref]$r) | Out-Null
    $p = 0
    [PW5]::GetWindowThreadProcessId($h, [ref]$p) | Out-Null
    [void]$rows.Add("hwnd=$h pid=$p x=$($r.L)..$($r.R) w=$($r.R-$r.L) y=$($r.T)..$($r.B)")
  }
  return $true
}
$gc = [System.Runtime.InteropServices.Marshal]::GetFunctionPointerForDelegate($cb)
[PW5]::EnumWindows($gc, [IntPtr]::Zero) | Out-Null
if ($rows.Count -eq 0) { 'no panel' } else { $rows }

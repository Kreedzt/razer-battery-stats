param()
Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
using System.Text;
public class MENU {
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool EnumWindows(IntPtr cb, IntPtr l);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern int GetWindowThreadProcessId(IntPtr h, out int pid);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  public struct RECT { public int L, T, R, B; }
  public delegate bool Cb(IntPtr h, IntPtr l);
}
"@
[MENU]::SetProcessDPIAware() | Out-Null
$target = (Get-Process razer-battery-taskbar -ErrorAction SilentlyContinue | Select-Object -First 1).Id
"panel process pid = $target"
$rows = New-Object System.Collections.ArrayList
$cb = [MENU+Cb]{ param($h, $l)
  $p = 0
  [MENU]::GetWindowThreadProcessId($h, [ref]$p) | Out-Null
  if ($p -eq $target -or $true) {
    $sb = New-Object System.Text.StringBuilder 256
    [MENU]::GetClassName($h, $sb, 256) | Out-Null
    $c = $sb.ToString()
    if ($c -eq '#32768' -or $c -eq 'RazerBatteryPanel') {
      $r = New-Object MENU+RECT
      [MENU]::GetWindowRect($h, [ref]$r) | Out-Null
      [void]$rows.Add(("pid={0,-6} {1,-20} vis={2,-5} x={3,5}..{4,-5} y={5,4}..{6,-4}" -f $p, $c, [MENU]::IsWindowVisible($h), $r.L, $r.R, $r.T, $r.B))
    }
  }
  return $true
}
$gc = [System.Runtime.InteropServices.Marshal]::GetFunctionPointerForDelegate($cb)
[MENU]::EnumWindows($gc, [IntPtr]::Zero) | Out-Null
if ($rows.Count -eq 0) { 'no panel/menu windows found' } else { $rows }

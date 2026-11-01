param()
Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
using System.Text;
public class GAP {
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindow(string c, string t);
  [DllImport("user32.dll")] public static extern bool EnumChildWindows(IntPtr p, IntPtr cb, IntPtr l);
  [DllImport("user32.dll")] public static extern IntPtr GetParent(IntPtr h);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern int GetWindowThreadProcessId(IntPtr h, out int pid);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern int GetWindowLong(IntPtr h, int i);
  public struct RECT { public int L, T, R, B; }
  public delegate bool Cb(IntPtr h, IntPtr l);
}
"@
[GAP]::SetProcessDPIAware() | Out-Null
$tray = [GAP]::FindWindow('Shell_TrayWnd', $null)
$rows = New-Object System.Collections.ArrayList
$cb = [GAP+Cb]{ param($h, $l)
  $r = New-Object GAP+RECT
  [GAP]::GetWindowRect($h, [ref]$r) | Out-Null
  # anything intersecting 2800..3200 horizontally
  if ($r.R -gt 2800 -and $r.L -lt 3200 -and $r.B -gt 0 -and $r.T -lt 70) {
    $sb = New-Object System.Text.StringBuilder 256
    [GAP]::GetClassName($h, $sb, 256) | Out-Null
    $tb = New-Object System.Text.StringBuilder 256
    [GAP]::GetWindowText($h, $tb, 256) | Out-Null
    $p = 0
    [GAP]::GetWindowThreadProcessId($h, [ref]$p) | Out-Null
    $par = [GAP]::GetParent($h)
    $psb = New-Object System.Text.StringBuilder 256
    if ($par -ne [IntPtr]::Zero) { [GAP]::GetClassName($par, $psb, 256) | Out-Null }
    [void]$rows.Add([pscustomobject]@{
      Class = $sb.ToString(); Parent = $psb.ToString(); L = $r.L; R = $r.R
      Vis = [GAP]::IsWindowVisible($h)
      Proc = (Get-Process -Id $p -ErrorAction SilentlyContinue).ProcessName
      Text = $tb.ToString(); Style = ('0x{0}' -f ([GAP]::GetWindowLong($h,-16)).ToString('X8'))
    })
  }
  return $true
}
$gc = [System.Runtime.InteropServices.Marshal]::GetFunctionPointerForDelegate($cb)
[GAP]::EnumChildWindows($tray, $gc, [IntPtr]::Zero) | Out-Null
$rows | Sort-Object L | Format-Table -AutoSize

param()
Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
using System.Text;
public class RB2 {
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindow(string c, string t);
  [DllImport("user32.dll")] public static extern bool EnumChildWindows(IntPtr p, IntPtr cb, IntPtr l);
  [DllImport("user32.dll")] public static extern IntPtr GetParent(IntPtr h);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern int GetWindowThreadProcessId(IntPtr h, out int pid);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  public struct RECT { public int L, T, R, B; }
  public delegate bool Cb(IntPtr h, IntPtr l);
}
"@
[RB2]::SetProcessDPIAware() | Out-Null
$script:rebar = [IntPtr]::Zero
$tray = [RB2]::FindWindow('Shell_TrayWnd', $null)
$rows = New-Object System.Collections.ArrayList
$cb = [RB2+Cb]{ param($h, $l)
  if ([RB2]::GetParent($h) -eq $script:rebar) {
    $sb = New-Object System.Text.StringBuilder 256
    [RB2]::GetClassName($h, $sb, 256) | Out-Null
    $r = New-Object RB2+RECT
    [RB2]::GetWindowRect($h, [ref]$r) | Out-Null
    $p = 0
    [RB2]::GetWindowThreadProcessId($h, [ref]$p) | Out-Null
    [void]$rows.Add([pscustomobject]@{
      Proc = (try { (Get-Process -Id $p -ErrorAction Stop).ProcessName } catch { '?' })
      Class = $sb.ToString(); L = $r.L; R = $r.R; W = ($r.R - $r.L)
      Vis = [RB2]::IsWindowVisible($h)
    })
  }
  return $true
}
$gc = [System.Runtime.InteropServices.Marshal]::GetFunctionPointerForDelegate($cb)
# locate rebar via first child walk
$script:any = [IntPtr]::Zero
$find = [RB2+Cb]{ param($h, $l)
  $sb = New-Object System.Text.StringBuilder 256
  [RB2]::GetClassName($h, $sb, 256) | Out-Null
  if ($sb.ToString() -eq 'ReBarWindow32' -and $script:rebar -eq [IntPtr]::Zero) { $script:rebar = $h }
  return $true
}
$gcf = [System.Runtime.InteropServices.Marshal]::GetFunctionPointerForDelegate($find)
[RB2]::EnumChildWindows($tray, $gcf, [IntPtr]::Zero) | Out-Null
"rebar = $script:rebar"
[RB2]::EnumChildWindows($tray, $gc, [IntPtr]::Zero) | Out-Null
$rows | Sort-Object L | Format-Table -AutoSize

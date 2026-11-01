# CLAUDE.md

Razer mouse battery panel for the Windows 10 classic taskbar. A resident window
reads battery level directly over HID — no Razer Synapse required.

## Stack

- Rust 2021, `windows` 0.61 (raw Win32; no GUI framework).
- `razer-hid` is a **git dependency pinned to a tag**:
  `{ git = "https://github.com/xzeldon/razer-battery-report", tag = "v1.3.0" }`
  (commit `42033bf`, MIT).
- That crate is not published to crates.io, but **GitHub is reachable and the git
  dependency resolves normally**; its own deps (`hidapi`, `thiserror`, `log`)
  come from crates.io. **Do not** reintroduce a vendored path dependency — that
  was an over-reaction to a mistaken "network unavailable" conclusion.

## Required checks

```
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release
```

All four must pass. `clippy -D warnings` is a hard gate.

`build.rs` compiles a `VERSIONINFO` resource with the Windows SDK's `rc.exe`.
It is pure std with zero build dependencies. **Do not** replace it with
`embed-resource`: that crate pulls `toml 0.9`/`winnow`, whose tarballs time out
on this machine (the index resolves, the download fails), which would make the
project unbuildable.

`rc.exe` lookup order: the `RC` environment variable, then
`Windows Kits\10\bin\<ver>\x64\rc.exe` picking the numerically newest SDK.
A missing compiler, an unrunnable one, or a non-zero exit each produce a
`cargo:warning` and the build continues without version metadata.
Override with `RC=/path/to/rc.exe`.

## Non-negotiable architecture

- **The panel must be a child of the rebar.** Create it as
  `WS_POPUP | WS_VISIBLE` with `WS_EX_TOOLWINDOW | WS_EX_LAYERED`, then
  `SetParent(panel, rebar)` followed by
  `SetLayeredWindowAttributes(panel, 0, 255, LWA_ALPHA)`. **Never** set
  `WS_EX_TOPMOST` or `WS_EX_NOACTIVATE`. This is TrafficMonitor's model and the
  only configuration that keeps drawing while the Start menu is open.
  - A topmost popup is excluded from DWM composition when Start/search opens:
    measured ink drops 1287 → 0 while `IsWindowVisible`, z-order enumeration and
    window rects all still look correct. Forcing `SetWindowPos(HWND_TOPMOST)`
    or toggling layered attributes afterwards does not help.
  - A plain `WS_CHILD` **without** `WS_EX_LAYERED` never composites across
    processes: `PrintWindow` renders content, the screen shows nothing.
- **The panel must be positioned in rebar client coordinates** (`MoveWindow`
  semantics). `GetWindowRect` returns screen coordinates; the two differ by the
  rebar's client origin (214 px on the reference machine).
- **A child needs no topmost watchdog and no WinEvent hook** — the parent owns
  the z-order. `FindWindowW` cannot find the panel either (it only walks
  top-level windows); use `FindWindowExW(rebar, …)` or keep the handle.
- **A process-owned host window is mandatory** (`RazerBatteryHost`, hidden
  top-level). A child window is destroyed with its parent, so after an Explorer
  restart **the panel window no longer exists**; re-embedding a stale `HWND`
  cannot recover it. The host owns `PanelState` (panel handle, monitor, timer)
  and calls `rebuild()` on `TaskbarCreated` or whenever the panel handle becomes
  invalid. Shutdown goes through the host's `WM_DESTROY`: `restore` the layout
  first, then destroy the panel.
- **`WM_PAINT` must bracket drawing with `BeginPaint`/`EndPaint`.** Without them
  the update region is never validated, `WM_PAINT` floods (observed: 790 queued
  messages) and starves `WM_TIMER`.
- **No HID I/O on the UI thread.** All device calls run on the `battery` worker;
  `snapshot()` takes the lock only after the worker has finished its I/O.
- **Never mix coordinate spaces.** The panel uses screen coordinates; the
  `MSTaskSwWClass` `MoveWindow` x (and every width) uses rebar client
  coordinates. All boundary maths is in screen coordinates.
- **Layout targets are absolute values** (`taskbar::plan` → `list_w` /
  `restored_w`). Callers **must not** subtract the panel width a second time:
  the watchdog recomputes every 2 s, so an incremental model subtracts once per
  tick (measured: task list collapsed from 2931 px to 178 px).
- **`Shell_TrayWnd` spans the whole screen and is not a taskbar boundary.**
  Derive the boundary from the smaller of `TrayNotifyWnd`'s left edge and
  `ReBarWindow32`'s right edge.
- **Reading semantics.** `0%` is a valid reading; no reading renders `--%`;
  disconnection must clear the value; charging is shown only when the device
  explicitly reports it (absence of a bolt does not mean "not charging").
- **The context menu must not overlap the taskbar.** Explorer will not repaint
  that area, cross-process `RedrawWindow` does not fix it, and the menu leaves
  ghost pixels behind.
- **Single instance is enforced by a named mutex** (`CreateMutexW(Global\…)`).
  A second instance reserves the strip again and shrinks the task buttons twice.
- **Auto-hide is tracked via the strip rect.** Auto-hide moves `Shell_TrayWnd`
  off screen instead of hiding it, so `IsWindowVisible` stays true; compare the
  visible area of the strip against the monitor rect instead.

## Pitfalls

- Before pixel-level verification, confirm the foreground is not
  `LockApp`/`LockScreenBackstopFrame`. While locked, every pixel check is a
  false negative.
- `FindWindowEx` only searches direct children. `DynamicContent2` and
  `TrayNotifyWnd` are children of `Shell_TrayWnd`, not of the rebar.
- In windows-rs 0.61, test GDI handles with `is_invalid()` (not `is_null()`);
  `Polygon` takes no brush argument, so `SelectObject` the brush first.
- For HID debugging, `razer-battery-report.exe status --json` (in `tools/`,
  gitignored).
- **The Start menu swallows third-party top-level windows.** Opening Start or
  search makes DWM drop every non-explorer top-level window from composition.
  Verify with pixel sampling (`BitBlt` the panel area and count dark pixels);
  `IsWindowVisible` and z-order enumeration still report everything as fine.
- **A child window dies with its parent.** After `SetParent(panel, rebar)`,
  restarting Explorer destroys the panel outright — the process survives with
  zero windows, and reusing the old `HWND` only yields an invalid handle.
  Rebuild through the host instead.
- **`SetParent` onto the rebar parks the window at the client origin.**
  `MoveWindow` takes client coordinates while `GetWindowRect` returns screen
  coordinates; mixing them shifts the panel by the rebar's left edge.

## Licensing

- The project is MIT; see `LICENSE`. Attribution for `razer-hid` and for the
  projects that informed the design lives in the README's Credits section.
- `razer-hid` is MIT: distributions must include its license text
  (<https://github.com/xzeldon/razer-battery-report/blob/v1.3.0/LICENSE>).
  `tools/` holds local scripts only — never third-party source.
- The binary embeds a `VERSIONINFO` resource (`ProductName`, `FileVersion`,
  `LegalCopyright` carrying the MIT attribution), fully readable via
  `GetFileVersionInfoSizeW` + `VerQueryValueW` (measured size 1980, translation
  `0x409/0x4B0`). It does **not** contain the full license text, so ship
  `LICENSE` alongside the executable.

## Working notes

`docs/` is gitignored on purpose: it holds research, task logs and screenshots
that are working material, not deliverables. Keeping it untracked keeps the
published repository to code, build config and licensing.

## Measured baseline (Win10 19045, 3840×2160 @200%)

| Metric | Value |
| --- | --- |
| Startup → first valid reading | ~410 ms (HID init 177 / enum 311 / read 383 + window 27) |
| Idle CPU | 0.000% (0 ms over 20 s) |
| CPU per poll | ~15.6 ms (31.3 ms over 65 s ≈ 0.048%) |
| Memory | 13.5 MB working set / 3.0 MB private |

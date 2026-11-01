//! Taskbar discovery and layout embedding for the classic (Windows 10) taskbar.
//!
//! Window chain: `Shell_TrayWnd` -> `ReBarWindow32` -> `MSTaskSwWClass`.
//! Embedding shrinks the task-button area (`MSTaskSwWClass`) by the panel width
//! and places the panel as another child of `ReBarWindow32` inside the freed
//! gap, to the left of the notification area.

use windows::Win32::Foundation::{HWND, POINT, RECT};

/// Handles of the classic taskbar window chain.
pub struct Taskbar {
    pub tray: HWND,
    pub rebar: HWND,
    pub mstask: HWND,
}

/// Total panel width that exactly fits the glyph and a text of `text_w`
/// pixels, with symmetric padding: `pad | glyph | gap | text | pad`.
///
/// Replaces the fixed width that left ~27 px of dead space on each side.
/// All arguments and the result are physical pixels for one taskbar height.
pub fn content_width(h: i32, text_w: i32) -> i32 {
    let pad = (h / 10).max(3);
    let gap = (h / 6).max(3);
    let glyph_w = glyph_width(h);
    pad + glyph_w + gap + text_w + pad
}

/// Width of the battery glyph (body + terminal nub) for a taskbar of `h`.
///
/// Horizontal proportions matching the Windows battery indicator: body width ≈
/// 1.5× its height with a small nub on the right. A portrait/1:2 body read as a
/// stretched black bar rather than a battery.
pub fn glyph_width(h: i32) -> i32 {
    let margin = (h / 12).max(2);
    let body_h = ((h - 2 * margin) * 7 / 10).max(4);
    let body_w = (body_h * 3 / 2).max(8);
    let nub = (body_h / 6).clamp(1, 3);
    body_w + nub
}

/// Target widths for one taskbar state, in screen x.
///
/// Pure geometry: `plan` is called on every watchdog tick, so it must be a
/// function of *stable* inputs only. Both fields are absolute targets, never a
/// delta applied to the current width — a delta would compound every tick and
/// eat the task-button area.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Plan {
    /// Width the task list must have once the panel is reserved.
    list_w: i32,
    /// Width the task list must have with no panel (giving the strip back).
    restored_w: i32,
}

/// Plans both widths from the list's left edge and the panel slot's right limit.
///
/// `slot_right` is the screen x where the reserved strip ends (the notification
/// area's left edge), `panel_w` the panel's width. The panel sits in
/// `[slot_right - gap - panel_w, slot_right - gap)` and the list ends one gap
/// before it, so no dead taskbar is left between the two.
fn plan(list_left: i32, slot_right: i32, panel_w: i32) -> Option<Plan> {
    let slot_left = slot_right - PANEL_GAP - panel_w;
    let list_w = slot_left - PANEL_GAP - list_left;
    let restored_w = slot_right - PANEL_GAP - list_left;
    if list_w <= 0 || restored_w <= list_w {
        return None;
    }
    Some(Plan { list_w, restored_w })
}

/// Where the panel goes and how wide the task list may stay.
///
/// Model: the panel is pinned to the **left edge of the notification area**
/// (`TrayNotifyWnd`) and the task list grows to touch it. That keeps the strip
/// seamless — anything else (anchoring the panel to the list, or leaving the
/// list short and parking the panel after some other tool) leaves a visible dead
/// gap, because Explorer reserves space for its own invisible band windows.
///
/// `limit` is the screen-space x where the panel slot starts; `natural` is the
/// task list's width that ends exactly there.
struct Layout {
    list_x: i32,
    list_w: i32,
    restored_w: i32,
    /// Screen x of the panel slot. Kept for diagnostics and hit testing.
    slot_x: i32,
    /// Panel x in the rebar's client space, which is what `MoveWindow` needs
    /// once the panel is parented to the rebar (TrafficMonitor model).
    slot_x_client: i32,
    slot_h: i32,
    slot_top: i32,
}

unsafe fn layout(tb: &Taskbar, panel: HWND, panel_w: i32) -> Option<Layout> {
    // `tb.tray` is `Shell_TrayWnd`, which spans the *whole screen*, so its right
    // edge is not a taskbar boundary. Boundaries come from the notification area
    // and from foreign tools, in screen coordinates. Only the list's position is
    // read in rebar client space, because `MoveWindow` takes parent client space.
    let rebar = rect_in_screen(tb.rebar).ok()?;
    let list = rect_in_screen(tb.mstask).ok()?;
    let list_client = rect_in_client(tb.mstask, tb.rebar).ok()?;
    let list_x = list_client.left;
    // Client origin of the rebar in screen space, needed to place the panel
    // (a rebar child) with `MoveWindow`.
    let rebar_client_left = {
        let mut pt = POINT { x: 0, y: 0 };
        if !ClientToScreen(tb.rebar, &mut pt).as_bool() {
            return None;
        }
        pt.x
    };

    let mut slot_right = rebar.right;
    if let Ok(h) = FindWindowExW(Some(tb.tray), None, w!("TrayNotifyWnd"), None) {
        if IsWindowVisible(h).as_bool() {
            if let Ok(r) = rect_in_screen(h) {
                slot_right = slot_right.min(r.left);
            }
        }
    }
    for o in foreign_strips(tb, panel) {
        // Foreign tools are *followers*: TrafficMonitor re-anchors itself to the
        // right edge of the task list, so using its current position as a fixed
        // obstacle would shrink the list again on every watchdog tick. Only a
        // band that is not currently sitting where our own slot goes constrains
        // us, and only when it is entirely right of the reservation.
        if o.right - o.left > 8 && o.left > slot_right {
            slot_right = slot_right.min(o.left);
        }
    }

    let p = plan(list.left, slot_right, panel_w)?;
    let slot_x = slot_right - PANEL_GAP - panel_w;
    // The panel is a child of the rebar, so its position is parent-relative.
    // The vertical offset is the same in screen and client space (the bar is at
    // the screen top), which the `slot_top` value already accounts for.
    let slot_x_client = slot_x - rebar_client_left;
    Some(Layout {
        list_x,
        list_w: p.list_w,
        restored_w: p.restored_w,
        slot_x,
        slot_x_client,
        slot_h: list.bottom - list.top,
        slot_top: list.top,
    })
}

/// Screen-coordinate rects of every visible *foreign* window inside the taskbar
/// strip, excluding this panel and the panel's own process.
///
/// A whole-desktop enumeration is required: TrafficMonitor's taskbar window is a
/// `#32770` whose parent is its own top-level host (`TrafficMonitor_xxxx`), which
/// has no taskbar parent at all — any subtree scan of `Shell_TrayWnd` misses it.
unsafe fn foreign_strips(tb: &Taskbar, panel: HWND) -> Vec<RECT> {
    let Ok(strip) = rect_in_screen(tb.tray) else {
        return Vec::new();
    };
    let mut explorer_pid = 0u32;
    GetWindowThreadProcessId(tb.tray, Some(&mut explorer_pid));
    let mut out = Vec::new();
    let mut next = None;
    loop {
        let cur = match FindWindowExW(None, next, None, None) {
            Ok(h) => h,
            Err(_) => break,
        };
        next = Some(cur);
        if cur == panel || !IsWindowVisible(cur).as_bool() {
            continue;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(cur, Some(&mut pid));
        if pid == explorer_pid {
            continue; // Explorer's own bands are not obstacles.
        }
        let Ok(o) = rect_in_screen(cur) else {
            continue;
        };
        if o.right <= o.left || o.bottom <= o.top {
            continue;
        }
        // Only windows lying inside the taskbar strip matter.
        if o.left < strip.left || o.right > strip.right || o.top < 0 {
            continue;
        }
        if o.bottom > strip.bottom + 8 {
            continue;
        }
        out.push(o);
    }
    out
}

/// Whether the taskbar strip is on screen, for auto-hide handling.
///
/// Auto-hide slides `Shell_TrayWnd` off the screen edge (top bar: `bottom <=
/// monitor top`) instead of hiding the window, so `IsWindowVisible` stays true
/// and a topmost panel would keep floating over the desktop. Comparing the
/// strip's rect against the monitor work area is the observable signal.
///
/// A taskbar that is merely 1-2 px clipped still counts as shown: Explorer
/// leaves a reveal sliver when the pointer leaves, and hiding the panel for it
/// would make the face flicker.
pub fn strip_visible(strip: RECT, monitor: RECT) -> bool {
    // Compare visible area, not edges: a 2 px reveal sliver is still "shown",
    // while a strip parked entirely outside the monitor is not.
    let vis_w = strip.right.min(monitor.right) - strip.left.max(monitor.left);
    let vis_h = strip.bottom.min(monitor.bottom) - strip.top.max(monitor.top);
    vis_w > 2 && vis_h > 2
}

/// Whether the current task-list width matches `natural` within tolerance.
fn width_ok(cur_w: i32, natural: i32) -> bool {
    (cur_w - natural).abs() <= PIN_TOL
}

/// Locates the classic taskbar window chain.
pub fn find_taskbar() -> Result<Taskbar, String> {
    unsafe {
        let tray = FindWindowW(w!("Shell_TrayWnd"), None).map_err(|_| "Shell_TrayWnd not found")?;
        let rebar = FindWindowExW(Some(tray), None, w!("ReBarWindow32"), None)
            .map_err(|_| "ReBarWindow32 not found under Shell_TrayWnd")?;
        let mstask = FindWindowExW(Some(rebar), None, w!("MSTaskSwWClass"), None)
            .map_err(|_| "MSTaskSwWClass not found under ReBarWindow32")?;
        Ok(Taskbar {
            tray,
            rebar,
            mstask,
        })
    }
}

/// Converts `hwnd`'s window rect into `client_of` client coordinates.
unsafe fn rect_in_client(hwnd: HWND, client_of: HWND) -> Result<RECT, String> {
    let mut r = RECT::default();
    GetWindowRect(hwnd, &mut r).map_err(|e| format!("GetWindowRect failed: {e}"))?;
    let mut pt = POINT {
        x: r.left,
        y: r.top,
    };
    ScreenToClient(client_of, &mut pt)
        .ok()
        .map_err(|e| format!("ScreenToClient failed: {e}"))?;
    Ok(RECT {
        left: pt.x,
        top: pt.y,
        right: pt.x + r.right - r.left,
        bottom: pt.y + r.bottom - r.top,
    })
}

/// Raw screen-coordinate rect of `hwnd`.
pub unsafe fn rect_in_screen(hwnd: HWND) -> Result<RECT, String> {
    let mut r = RECT::default();
    GetWindowRect(hwnd, &mut r).map_err(|e| format!("GetWindowRect failed: {e}"))?;
    Ok(r)
}

/// Gap between the task list and the panel (TrafficMonitor uses 2 px).
pub const PANEL_GAP: i32 = 2;

/// Panel position tolerance before the watchdog re-pins it (rounding).
pub const PIN_TOL: i32 = 2;

/// Reserves `panel_w` by shrinking the task list and pins `panel` in the strip.
///
/// Widths come straight from [`Plan`]; nothing here re-derives them, so calling
/// this twice cannot reserve twice.
pub fn embed(panel: HWND, tb: &Taskbar, panel_w: i32) -> Result<(), String> {
    unsafe {
        let l = layout(tb, panel, panel_w)
            .ok_or_else(|| "no space left of the notification area".to_string())?;
        let r = rect_in_screen(tb.mstask)?;
        MoveWindow(tb.mstask, l.list_x, r.top, l.list_w, r.bottom - r.top, true)
            .map_err(|e| format!("MoveWindow(mstask) failed: {e}"))?;
        if let Err(e) = MoveWindow(panel, l.slot_x_client, l.slot_top, panel_w, l.slot_h, true) {
            // Roll the list back so a failed embed leaves no trace.
            let _ = MoveWindow(
                tb.mstask,
                l.list_x,
                r.top,
                l.restored_w,
                r.bottom - r.top,
                true,
            );
            return Err(format!("MoveWindow(panel) failed: {e}"));
        }
        Ok(())
    }
}

/// Keeps the panel in its slot and the width reservation intact.
///
/// Both targets are absolute, so repeated watchdog calls converge instead of
/// compounding.
pub fn assert_layout(panel: HWND, tb: &Taskbar, panel_w: i32) {
    unsafe {
        if !IsWindow(Some(tb.rebar)).as_bool() || !IsWindow(Some(tb.mstask)).as_bool() {
            return; // Explorer gone; TaskbarCreated handling re-embeds later.
        }
        let Some(l) = layout(tb, panel, panel_w) else {
            return;
        };
        let Ok(r) = rect_in_screen(tb.mstask) else {
            return;
        };
        if !width_ok(r.right - r.left, l.list_w) {
            let _ = MoveWindow(tb.mstask, l.list_x, r.top, l.list_w, r.bottom - r.top, true);
        }
        let Ok(p) = rect_in_screen(panel) else {
            return;
        };
        if p.left != l.slot_x
            || p.top != l.slot_top
            || p.right - p.left != panel_w
            || p.bottom - p.top != l.slot_h
        {
            let _ = MoveWindow(panel, l.slot_x_client, l.slot_top, panel_w, l.slot_h, true);
        }
    }
}

/// Gives the reserved strip back to the task-button area.
///
/// The target width is recomputed from the current layout, so leftovers from a
/// force-killed instance are healed instead of stacked. Handles are re-validated
/// first: after an Explorer restart the old HWNDs are meaningless.
pub fn restore(panel: HWND, tb: &Taskbar, panel_w: i32) {
    unsafe {
        if !IsWindow(Some(tb.mstask)).as_bool() {
            return;
        }
        let Some(l) = layout(tb, panel, panel_w) else {
            return;
        };
        let Ok(r) = rect_in_screen(tb.mstask) else {
            return;
        };
        if !width_ok(r.right - r.left, l.restored_w) {
            let _ = MoveWindow(
                tb.mstask,
                l.list_x,
                r.top,
                l.restored_w,
                r.bottom - r.top,
                true,
            );
        }
    }
}

use windows::core::w;
use windows::Win32::Graphics::Gdi::{ClientToScreen, ScreenToClient};

use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowExW, FindWindowW, GetWindowRect, GetWindowThreadProcessId, IsWindow, IsWindowVisible,
    MoveWindow,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_width_has_no_dead_space() {
        // At a 60 px taskbar the glyph is ~44 px wide; a 100 % label in the
        // panel font measures roughly 50 px. The total must be exactly its
        // parts, so the window fits the content with symmetric padding.
        let h = 60;
        let text_w = 50;
        let w = content_width(h, text_w);
        let pad = (h / 10).max(3);
        let gap = (h / 6).max(3);
        assert_eq!(w, pad + glyph_width(h) + gap + text_w + pad);
        assert!(w < 150, "panel should stay compact, got {w}");
        assert!(w > text_w + glyph_width(h), "panel must fit both parts");
    }

    #[test]
    fn glyph_width_grows_with_height() {
        assert!(glyph_width(96) > glyph_width(60));
        assert!(glyph_width(60) > glyph_width(24));
        // Degenerate taskbars still get a drawable body + nub.
        assert!(glyph_width(4) >= 6);
    }

    #[test]
    fn plan_reserves_exactly_the_panel_strip() {
        // Tray at 3295, 126 px panel: panel 3167..3293, list ends at 3165 and
        // the strip is handed back up to 3293 when no panel is reserved.
        let p = plan(216, 3295, 126).expect("room for the panel");
        assert_eq!(p.list_w, 3165 - 216);
        assert_eq!(p.restored_w, 3293 - 216);
    }

    #[test]
    fn plan_is_idempotent_whatever_the_current_width() {
        // Absolute targets: a watchdog that recomputes every tick must never
        // compound the reservation.
        assert_eq!(plan(216, 3295, 126), plan(216, 3295, 126));
    }

    #[test]
    fn plan_rejects_a_panel_wider_than_the_strip() {
        assert!(plan(216, 3295, 4000).is_none());
        assert!(plan(3200, 3295, 126).is_none()); // list already past the slot
    }

    #[test]
    fn strip_visible_tracks_autohide_off_screen() {
        let monitor = RECT {
            left: 0,
            top: 0,
            right: 3840,
            bottom: 2160,
        };
        // Shown: the strip occupies the top edge of the monitor.
        assert!(strip_visible(
            RECT {
                left: 0,
                top: 0,
                right: 3840,
                bottom: 60
            },
            monitor
        ));
        // Auto-hidden upward: the whole strip sits above the monitor.
        assert!(!strip_visible(
            RECT {
                left: 0,
                top: -60,
                right: 3840,
                bottom: 0
            },
            monitor
        ));
        // A small reveal sliver left by Explorer still counts as shown.
        assert!(strip_visible(
            RECT {
                left: 0,
                top: -52,
                right: 3840,
                bottom: 8
            },
            monitor
        ));
        // Bottom-docked taskbar hidden downward.
        assert!(!strip_visible(
            RECT {
                left: 0,
                top: 2160,
                right: 3840,
                bottom: 2220
            },
            monitor
        ));
    }

    #[test]
    fn width_ok_accepts_only_the_target() {
        assert!(width_ok(2347, 2347));
        assert!(width_ok(2346, 2347)); // within tolerance
        assert!(!width_ok(2203, 2347)); // stale reservation from an old crash
        assert!(!width_ok(2491, 2347));
    }
}

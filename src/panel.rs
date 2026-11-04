//! The battery panel child window: double-buffered painting of a battery
//! glyph plus placeholder text, a periodic layout watchdog, and teardown that
//! restores the taskbar layout.
//!
//! CP2a draws a placeholder (`--%`, half-filled glyph); CP3 replaces it with
//! live readings from the HID worker.

use std::time::Instant;

use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, CreateFontW, CreatePen,
    CreateSolidBrush, DeleteDC, DeleteObject, DrawTextW, EndPaint, FillRect, GetDC,
    GetMonitorInfoW, GetPixel, GetStockObject, GetSysColor, GetSysColorBrush,
    GetTextExtentPoint32W, InvalidateRect, MonitorFromWindow, Polygon, Rectangle, RedrawWindow,
    ReleaseDC, SelectObject, SetBkMode, SetTextColor, CLEARTYPE_QUALITY, CLR_INVALID, COLOR_INFOBK,
    COLOR_INFOTEXT, DEFAULT_CHARSET, DT_CALCRECT, DT_LEFT, DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER,
    FONT_CLIP_PRECISION, FONT_OUTPUT_PRECISION, FW_NORMAL, FW_SEMIBOLD, HDC, MONITORINFO,
    MONITOR_DEFAULTTONEAREST, NULL_BRUSH, PAINTSTRUCT, PS_SOLID, RDW_ALLCHILDREN, RDW_ERASE,
    RDW_INVALIDATE, RDW_UPDATENOW, SRCCOPY, TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow,
    FindWindowW, GetClientRect, GetCursorPos, GetWindowLongPtrW, GetWindowRect, IsWindow,
    IsWindowVisible, KillTimer, PostQuitMessage, RegisterClassW, RegisterWindowMessageW,
    SetLayeredWindowAttributes, SetParent, SetTimer, SetWindowLongPtrW, SetWindowPos,
    SetWindowTextW, ShowWindow, TrackPopupMenu, CREATESTRUCTW, CS_HREDRAW, CS_VREDRAW,
    GWLP_USERDATA, HWND_TOPMOST, LAYERED_WINDOW_ATTRIBUTES_FLAGS, MF_CHECKED, MF_SEPARATOR,
    MF_STRING, SWP_NOACTIVATE, SW_HIDE, SW_SHOWNOACTIVATE, TPM_NONOTIFY, TPM_RETURNCMD,
    TPM_RIGHTBUTTON, WINDOW_EX_STYLE, WINDOW_STYLE, WM_CLOSE, WM_DESTROY, WM_NCCREATE, WM_TIMER,
    WNDCLASSW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_POPUP, WS_VISIBLE,
};

use crate::taskbar::Taskbar;
use windows::Win32::System::LibraryLoader::GetModuleFileNameW;

/// The hidden, process-owned host window class.
const HOST_CLASS_NAME: PCWSTR = w!("RazerBatteryHost");
/// The visible panel class (a child of Explorer's rebar).
const PANEL_CLASS_NAME: PCWSTR = w!("RazerBatteryPanel");
/// The hover tooltip class (a top-level, never-activated popup).
const TIP_CLASS_NAME: PCWSTR = w!("RazerBatteryTooltip");
/// Layout/repaint cadence (also the HID-independent refresh of the face).
const TIMER_ID: usize = 1;
const TIMER_MS: u32 = 2000;
/// Polls the cursor while the tooltip is up; there is no `WM_MOUSELEAVE`
/// without `TrackMouseEvent`, and this is cheaper than adding that feature.
const TIP_TIMER_ID: usize = 2;
const TIP_TIMER_MS: u32 = 150;

/// Program state, owned by the *host* window via `GWLP_USERDATA`.
///
/// The host is a plain hidden window owned by this process. It exists because
/// the panel must be a child of Explorer's rebar (the only way to stay composed
/// under the Start menu), and a child window is destroyed when its parent dies.
/// Explorer restarting would therefore kill the panel; the host outlives it and
/// builds a fresh panel after each `TaskbarCreated`.
pub struct PanelState {
    tb: Taskbar,
    panel_w: i32,
    text_color: COLORREF,
    monitor: Option<crate::battery::BatteryMonitor>,
    /// The current panel window, or null between an Explorer restart and the
    /// rebuild. Never a stale handle: it is cleared on `WM_DESTROY`.
    panel: HWND,
    /// The hover tooltip, created on first hover and kept across panel
    /// rebuilds (it is a top-level window, not a child of the rebar).
    tip: HWND,
    /// Text currently painted in the tooltip and the snapshot it was built
    /// from, so a repaint never re-measures different text.
    tip_text: String,
    tip_snapshot: Option<crate::battery::Snapshot>,
    /// Panel screen rect the tooltip was last anchored to; a watchdog re-embed
    /// or tray reflow moves the panel and the tip must follow.
    tip_panel_rect: RECT,
}

/// Label sampled at startup to size the window: a typical two-digit reading.
/// The panel is snug for it, and grows once if a real reading needs more room
/// (100 %) — instead of reserving space for the worst case at all times.
pub const TYPICAL_LABEL: &str = "88%";

/// Measures a label in the same font the panel paints with, at a given panel
/// height. Returns the glyph width needed for `text` in pixels.
pub(crate) unsafe fn measure_label(text: &str, h: i32) -> i32 {
    let hdc = GetDC(None);
    if hdc.is_invalid() {
        return 0;
    }
    let font = CreateFontW(
        -(h * 2 / 5),
        0,
        0,
        0,
        FW_SEMIBOLD.0 as i32,
        0,
        0,
        0,
        DEFAULT_CHARSET,
        FONT_OUTPUT_PRECISION(0),
        FONT_CLIP_PRECISION(0),
        CLEARTYPE_QUALITY,
        0,
        w!("Segoe UI"),
    );
    let mut width = 0;
    if !font.is_invalid() {
        let old = SelectObject(hdc, font.into());
        let wide: Vec<u16> = text.encode_utf16().collect();
        let mut size = SIZE::default();
        if GetTextExtentPoint32W(hdc, &wide, &mut size).as_bool() {
            width = size.cx;
        }
        SelectObject(hdc, old);
        let _ = DeleteObject(font.into());
    }
    let _ = ReleaseDC(None, hdc);
    width
}

/// Creates the long-lived host window and the first panel.
///
/// Fails (no silent fallback to a floating window) when the taskbar cannot be
/// found or the layout cannot be applied.
pub fn create(tb: &Taskbar, panel_w: i32) -> Result<HWND, String> {
    unsafe {
        let hinstance = GetModuleHandleW(None).map_err(|e| format!("GetModuleHandleW: {e}"))?;
        let host_class = WNDCLASSW {
            lpfnWndProc: Some(host_wndproc),
            hInstance: hinstance.into(),
            lpszClassName: HOST_CLASS_NAME,
            ..Default::default()
        };
        if RegisterClassW(&host_class) == 0 {
            return Err("RegisterClassW(host) failed".into());
        }
        let panel_class = WNDCLASSW {
            lpfnWndProc: Some(panel_wndproc),
            hInstance: hinstance.into(),
            lpszClassName: PANEL_CLASS_NAME,
            style: CS_HREDRAW | CS_VREDRAW, // redraw on width/height changes
            ..Default::default()
        };
        if RegisterClassW(&panel_class) == 0 {
            return Err("RegisterClassW(panel) failed".into());
        }
        // A normal tooltip surface: the system info colours stay readable in
        // both light and dark themes, and the class brush erases the popup.
        let tip_class = WNDCLASSW {
            lpfnWndProc: Some(tip_wndproc),
            hInstance: hinstance.into(),
            lpszClassName: TIP_CLASS_NAME,
            hbrBackground: GetSysColorBrush(COLOR_INFOBK),
            ..Default::default()
        };
        if RegisterClassW(&tip_class) == 0 {
            return Err("RegisterClassW(tooltip) failed".into());
        }
        let state = Box::new(PanelState {
            tb: Taskbar {
                tray: tb.tray,
                rebar: tb.rebar,
                mstask: tb.mstask,
            },
            panel_w,
            text_color: theme_text_color(),
            monitor: None,
            panel: HWND::default(),
            tip: HWND::default(),
            tip_text: String::new(),
            tip_snapshot: None,
            tip_panel_rect: RECT::default(),
        });
        // The host is a hidden top-level window owned by this process only. It
        // must NOT be a child of the taskbar: it has to survive Explorer exiting.
        let host = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            HOST_CLASS_NAME,
            w!("Razer Battery Host"),
            WINDOW_STYLE(0), // plain top-level; never shown.
            0,
            0,
            0,
            0,
            None,
            None,
            Some(hinstance.into()),
            Some(Box::into_raw(state) as *mut _),
        )
        .map_err(|e| format!("CreateWindowExW(host) failed: {e}"))?;
        let st = &mut *state_ptr(host);
        let panel = build_panel(host, st)?;
        st.monitor = Some(crate::battery::BatteryMonitor::start(host));
        SetTimer(Some(host), TIMER_ID, TIMER_MS, None);
        let _ = panel;
        Ok(host)
    }
}

/// Creates the panel window, parents it to the current rebar and reserves its
/// strip. Returns the new panel handle.
///
/// Called once at startup and again after every `TaskbarCreated`: a child
/// window dies with its parent, so a rebuild is the only correct response to an
/// Explorer restart.
unsafe fn build_panel(host: HWND, st: &mut PanelState) -> Result<HWND, String> {
    let hinstance = GetModuleHandleW(None).map_err(|e| format!("GetModuleHandleW: {e}"))?;
    let panel = CreateWindowExW(
        // WS_EX_TOOLWINDOW | WS_EX_LAYERED (0x80000).
        //
        // The TrafficMonitor model, verified against the Start menu: the panel
        // is a *child of the rebar* and layered, with no WS_EX_TOPMOST and no
        // WS_EX_NOACTIVATE. A topmost popup is excluded from DWM composition
        // while Start is open (measured: 1287 ink pixels -> 0), whereas this
        // configuration keeps drawing (verified across repeated open/close).
        // `WS_EX_LAYERED` is what makes the cross-process child composite at
        // all; without it the rebar never paints us.
        WS_EX_TOOLWINDOW | WINDOW_EX_STYLE(0x0008_0000),
        PANEL_CLASS_NAME,
        w!("Razer Battery"),
        WS_POPUP | WS_VISIBLE,
        0,
        0,
        st.panel_w,
        10, // final geometry set by taskbar::embed
        None,
        None,
        Some(hinstance.into()),
        Some(host.0 as *mut _), // GWLP_USERDATA = the host, not a state box
    )
    .map_err(|e| format!("CreateWindowExW(panel) failed: {e}"))?;
    // Parenting to the rebar keeps the panel composed while the Start menu is
    // open. Must precede `embed`, which positions it in rebar client space.
    if SetParent(panel, Some(st.tb.rebar)).is_err() {
        let _ = DestroyWindow(panel);
        return Err("SetParent(panel -> rebar) failed".into());
    }
    // A layered child needs an explicit alpha or it composites as fully
    // transparent (screen pixels stay empty while PrintWindow has content).
    let _ = SetLayeredWindowAttributes(
        panel,
        COLORREF(0),
        255,
        LAYERED_WINDOW_ATTRIBUTES_FLAGS(0x2), // LWA_ALPHA
    );
    if let Err(e) = crate::taskbar::embed(panel, &st.tb, st.panel_w) {
        let _ = DestroyWindow(panel);
        return Err(e);
    }
    st.panel = panel;
    paint(panel);
    Ok(panel)
}

/// Creates the hidden tooltip popup on first hover.
unsafe fn create_tip(host: HWND) -> Result<HWND, String> {
    let hinstance = GetModuleHandleW(None).map_err(|e| format!("GetModuleHandleW: {e}"))?;
    CreateWindowExW(
        WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
        TIP_CLASS_NAME,
        w!(""),
        WS_POPUP,
        0,
        0,
        0,
        0,
        None,
        None,
        Some(hinstance.into()),
        Some(host.0 as *mut _), // GWLP_USERDATA = the host, not a state box
    )
    .map_err(|e| format!("CreateWindowExW(tooltip) failed: {e}"))
}

/// Shows the tooltip on hover, or keeps it up while the cursor moves inside.
unsafe fn hover_tip(panel: HWND) {
    let host = host_of(panel);
    if host.is_invalid() {
        return;
    }
    let st = &mut *state_ptr(host);
    if st.tip.is_invalid() {
        match create_tip(host) {
            Ok(tip) => st.tip = tip,
            Err(_) => return,
        }
    }
    if !IsWindowVisible(st.tip).as_bool() {
        update_tip(panel, st);
        // Arm once per hover. `SetTimer` with an existing id restarts the
        // deadline, so re-arming on every mouse move would starve the 150 ms
        // refresh/hide tick while the cursor keeps moving.
        SetTimer(Some(panel), TIP_TIMER_ID, TIP_TIMER_MS, None);
    }
}

/// Refreshes the tooltip while it is up or re-shows it after a transient hide.
///
/// The snapshot gate alone is not enough: a failed or missing mouse does not
/// change the snapshot on every poll, yet "Last read N ago" must keep growing;
/// and the panel can move under the cursor (watchdog re-embed, tray reflow), so
/// the tip has to follow its anchor. The rendered text is compared instead of
/// rebuilding on every 150 ms tick.
unsafe fn refresh_tip(panel: HWND) {
    let host = host_of(panel);
    if host.is_invalid() {
        return;
    }
    let st = &mut *state_ptr(host);
    if st.tip.is_invalid() {
        return;
    }
    let snapshot = st
        .monitor
        .as_ref()
        .map(crate::battery::BatteryMonitor::snapshot)
        .unwrap_or_default();
    let text = crate::battery::details_text(&snapshot, Instant::now());
    let mut panel_rc = RECT::default();
    let moved = GetWindowRect(panel, &mut panel_rc).is_ok() && panel_rc != st.tip_panel_rect;
    // Re-show whenever the window is not visible: a repaint or a transient
    // hide must not leave the hover with no tooltip until the next poll.
    if !IsWindowVisible(st.tip).as_bool()
        || moved
        || st.tip_snapshot.as_ref() != Some(&snapshot)
        || st.tip_text != text
    {
        update_tip(panel, st);
    }
}

/// Sizes, positions and paints the tooltip from the current snapshot.
unsafe fn update_tip(panel: HWND, st: &mut PanelState) {
    if st.tip.is_invalid() {
        return;
    }
    let snapshot = st
        .monitor
        .as_ref()
        .map(crate::battery::BatteryMonitor::snapshot)
        .unwrap_or_default();
    let text = crate::battery::details_text(&snapshot, Instant::now());
    let dpi = tip_dpi(panel);
    let (pad_x, pad_y) = tip_padding(dpi);

    // Measure with the same font the paint uses; `DT_CALCRECT` with no
    // `DT_WORDBREAK` reports the widest line and the total line height.
    let hdc = GetDC(Some(st.tip));
    if hdc.is_invalid() {
        return;
    }
    let font = tip_font(dpi);
    let old = if font.is_invalid() {
        None
    } else {
        Some(SelectObject(hdc, font.into()))
    };
    let mut wide: Vec<u16> = text.encode_utf16().collect();
    let mut rc = RECT {
        left: 0,
        top: 0,
        right: 1000,
        bottom: 0,
    };
    let _ = DrawTextW(hdc, &mut wide, &mut rc, DT_CALCRECT | DT_LEFT | DT_NOPREFIX);
    if let Some(old) = old {
        SelectObject(hdc, old);
        let _ = DeleteObject(font.into());
    }
    let _ = ReleaseDC(Some(st.tip), hdc);

    let w = (rc.right - rc.left).max(1) + 2 * pad_x;
    let h = (rc.bottom - rc.top).max(1) + 2 * pad_y;

    let mut panel_rc = RECT::default();
    if GetWindowRect(panel, &mut panel_rc).is_err() {
        return;
    }
    let monitor = MonitorFromWindow(panel, MONITOR_DEFAULTTONEAREST);
    let mut mi = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if !GetMonitorInfoW(monitor, &mut mi).as_bool() {
        return;
    }
    let bounds = mi.rcMonitor;
    let gap = (dpi * 4 / 96).max(2);
    // Below the panel is outside the bar for a top taskbar; above it is the
    // only option for a bottom one. The cursor is inside the bar, so it cannot
    // be used as the anchor.
    let below = panel_rc.bottom + gap;
    let above = panel_rc.top - gap - h;
    let y = if below + h <= bounds.bottom {
        below
    } else {
        above.max(bounds.top)
    };
    let x = panel_rc
        .left
        .clamp(bounds.left, (bounds.right - w).max(bounds.left));

    let _ = SetWindowPos(st.tip, Some(HWND_TOPMOST), x, y, w, h, SWP_NOACTIVATE);
    let _ = ShowWindow(st.tip, SW_SHOWNOACTIVATE);
    let _ = InvalidateRect(Some(st.tip), None, true);
    // The window title carries the same text: a custom-painted tooltip has no
    // accessible text otherwise, and diagnostic scripts can read it verbatim.
    let _ = SetWindowTextW(st.tip, &HSTRING::from(text.as_str()));
    st.tip_text = text;
    st.tip_snapshot = Some(snapshot);
    st.tip_panel_rect = panel_rc;
}

/// Hides the tooltip and stops the cursor poll. Safe on a dead panel.
unsafe fn hide_tip(panel: HWND) {
    let host = host_of(panel);
    if host.is_invalid() {
        return;
    }
    let st = &mut *state_ptr(host);
    if !st.tip.is_invalid() {
        let _ = ShowWindow(st.tip, SW_HIDE);
    }
    st.tip_snapshot = None;
    let _ = KillTimer(Some(panel), TIP_TIMER_ID);
}

/// Whether the cursor is still inside the panel's screen rect.
unsafe fn cursor_inside(panel: HWND) -> bool {
    let mut pt = POINT::default();
    let mut rc = RECT::default();
    if GetCursorPos(&mut pt).is_err() || GetWindowRect(panel, &mut rc).is_err() {
        return false;
    }
    pt.x >= rc.left && pt.x < rc.right && pt.y >= rc.top && pt.y < rc.bottom
}

/// DPI of `window`, clamped away from the documented 0 on failure.
unsafe fn tip_dpi(window: HWND) -> i32 {
    let dpi = GetDpiForWindow(window);
    if dpi == 0 {
        96
    } else {
        dpi as i32
    }
}

/// Padding around the tooltip text, scaled with the monitor DPI.
fn tip_padding(dpi: i32) -> (i32, i32) {
    ((dpi * 12 / 96).max(8), (dpi * 8 / 96).max(6))
}

/// Tooltip font: 9 pt Segoe UI, the Windows UI face.
unsafe fn tip_font(dpi: i32) -> windows::Win32::Graphics::Gdi::HFONT {
    CreateFontW(
        -(dpi * 9 / 72),
        0,
        0,
        0,
        FW_NORMAL.0 as i32,
        0,
        0,
        0,
        DEFAULT_CHARSET,
        FONT_OUTPUT_PRECISION(0),
        FONT_CLIP_PRECISION(0),
        CLEARTYPE_QUALITY,
        0,
        w!("Segoe UI"),
    )
}

/// Window procedure of the tooltip: paint only.
unsafe extern "system" fn tip_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_NCCREATE => {
            let cs = &*(lparam.0 as *const CREATESTRUCTW);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, cs.lpCreateParams as isize);
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        // WM_PAINT = 0x000F; BeginPaint/EndPaint are paired for the same
        // reason as the panel: an unvalidated update region floods the queue.
        0x000F => {
            let mut ps = PAINTSTRUCT::default();
            let hdc = BeginPaint(hwnd, &mut ps);
            let host = HWND(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut _);
            if !host.is_invalid() {
                let st = &*state_ptr(host);
                let dpi = tip_dpi(hwnd);
                let (pad_x, pad_y) = tip_padding(dpi);
                let mut client = RECT::default();
                let _ = GetClientRect(hwnd, &mut client);
                let font = tip_font(dpi);
                let old = if font.is_invalid() {
                    None
                } else {
                    Some(SelectObject(hdc, font.into()))
                };
                let _ = SetBkMode(hdc, TRANSPARENT);
                let prev = SetTextColor(hdc, COLORREF(GetSysColor(COLOR_INFOTEXT)));
                let mut text: Vec<u16> = st.tip_text.encode_utf16().collect();
                let mut rc = RECT {
                    left: pad_x,
                    top: pad_y,
                    right: client.right - pad_x,
                    bottom: client.bottom - pad_y,
                };
                let _ = DrawTextW(hdc, &mut text, &mut rc, DT_LEFT | DT_NOPREFIX);
                SetTextColor(hdc, prev);
                if let Some(old) = old {
                    SelectObject(hdc, old);
                    let _ = DeleteObject(font.into());
                }
            }
            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

/// Fixed glyph/text color: black. The panel samples the taskbar pixels as its
/// background, and the Win10 taskbar is light in both light and dark modes
/// (dark mode tints it only slightly), so black stays readable either way.
/// Registry theme detection was tried and disagreed with the actual taskbar
/// color, so it was dropped (see task archive).
fn theme_text_color() -> COLORREF {
    COLORREF(0x0000_0000)
}

unsafe fn state_ptr(hwnd: HWND) -> *mut PanelState {
    GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut PanelState
}

/// Window procedure of the hidden host: owns the state, drives the panel.
unsafe extern "system" fn host_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_NCCREATE => {
            let cs = &*(lparam.0 as *const CREATESTRUCTW);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, cs.lpCreateParams as isize);
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_TIMER => {
            let st = &mut *state_ptr(hwnd);
            let panel = st.panel;
            if st.panel.is_invalid() || !IsWindow(Some(panel)).as_bool() {
                // The panel disappeared without a TaskbarCreated (Explorer died
                // in a way that never broadcast one). Rebuild from scratch.
                let _ = rebuild(hwnd);
                return LRESULT(0);
            }
            // Auto-hide slides the strip off screen without hiding its window,
            // so the panel would stay painted over the desktop. Follow the
            // strip: hide with it, reappear with it, and do not touch the task
            // list while it is off screen (its rect is meaningless there).
            if !follow_taskbar_visibility(panel, &st.tb) {
                return LRESULT(0);
            }
            crate::taskbar::assert_layout(panel, &st.tb, st.panel_w);
            paint(panel);
            LRESULT(0)
        }
        crate::battery::WM_BATTERY_UPDATED => {
            let st = &*state_ptr(hwnd);
            if !st.panel.is_invalid() {
                paint(st.panel);
            }
            LRESULT(0)
        }
        // Explorer was restarted. Everything anchored to the old Explorer is
        // gone: the panel was a child of its rebar, and a child window is
        // destroyed with its parent, so it must be rebuilt, not re-embedded.
        msg if msg == taskbar_created_message() => {
            let _ = rebuild(hwnd);
            LRESULT(0)
        }
        WM_DESTROY => {
            let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut PanelState;
            if !ptr.is_null() {
                let mut state = Box::from_raw(ptr);
                if let Some(mut monitor) = state.monitor.take() {
                    // Join off the window-proc path: the worker may be inside a
                    // HID timeout. It keeps running harmlessly until then.
                    std::thread::spawn(move || monitor.stop());
                }
                let _ = KillTimer(Some(hwnd), TIMER_ID);
                if !state.panel.is_invalid() {
                    // Give the strip back before the window goes away: the
                    // layout target is recomputed from the live taskbar.
                    crate::taskbar::restore(state.panel, &state.tb, state.panel_w);
                    let _ = DestroyWindow(state.panel);
                }
                if !state.tip.is_invalid() {
                    let _ = DestroyWindow(state.tip);
                }
                drop(state);
            }
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

/// Window procedure of the visible panel.
///
/// The panel owns no state: it forwards everything to the host, which is the
/// only window guaranteed to outlive an Explorer restart.
unsafe extern "system" fn panel_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_NCCREATE => {
            let cs = &*(lparam.0 as *const CREATESTRUCTW);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, cs.lpCreateParams as isize);
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        // WM_PAINT = 0x000F. BeginPaint/EndPaint must bracket the drawing to
        // validate the update region; painting without them re-queues WM_PAINT
        // forever (observed as a 790-message backlog that starved WM_TIMER).
        0x000F => {
            let mut ps = PAINTSTRUCT::default();
            let hdc = BeginPaint(hwnd, &mut ps);
            let mut rc = RECT::default();
            let _ = GetClientRect(hwnd, &mut rc);
            let w = rc.right - rc.left;
            let h = rc.bottom - rc.top;
            if w > 0 && h > 0 {
                let host = host_of(hwnd);
                let st = &*state_ptr(host);
                let snapshot = st
                    .monitor
                    .as_ref()
                    .map(crate::battery::BatteryMonitor::snapshot)
                    .unwrap_or_default();
                paint_into(hdc, w, h, st.text_color, &snapshot);
            }
            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        }
        // WM_MOUSEMOVE = 0x0200: show the details tooltip on hover.
        0x0200 => {
            hover_tip(hwnd);
            LRESULT(0)
        }
        // Hides the tooltip once the cursor leaves the panel. A timer is used
        // instead of WM_MOUSELEAVE because the latter needs TrackMouseEvent.
        WM_TIMER if wparam.0 == TIP_TIMER_ID => {
            if cursor_inside(hwnd) {
                refresh_tip(hwnd);
            } else {
                hide_tip(hwnd);
            }
            LRESULT(0)
        }
        // WM_RBUTTONUP = 0x0205: the panel's only user interaction.
        0x0205 => {
            hide_tip(hwnd);
            show_menu(hwnd);
            LRESULT(0)
        }
        WM_CLOSE => {
            // Ask the host to shut the whole program down.
            let _ = DestroyWindow(host_of(hwnd));
            LRESULT(0)
        }
        WM_DESTROY => {
            // Clear the host's handle so it never acts on a dead window. The
            // host itself stays alive (it may be mid-rebuild).
            hide_tip(hwnd);
            let host = host_of(hwnd);
            if !host.is_invalid() {
                let ptr = GetWindowLongPtrW(host, GWLP_USERDATA) as *mut PanelState;
                if !ptr.is_null() && (*ptr).panel == hwnd {
                    (*ptr).panel = HWND::default();
                }
            }
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

/// The host window that owns `panel` (`GWLP_USERDATA` of a panel is its host).
fn host_of(panel: HWND) -> HWND {
    unsafe { HWND(GetWindowLongPtrW(panel, GWLP_USERDATA) as *mut _) }
}

/// Re-discovers the taskbar, then destroys and recreates the panel.
///
/// A child window cannot outlive its parent, so after an Explorer restart the
/// old panel handle is not merely stale — the window is gone. Rebuilding gives
/// a panel parented to the *new* rebar with correct geometry.
unsafe fn rebuild(host: HWND) -> Result<(), String> {
    let st = &mut *state_ptr(host);
    if !st.panel.is_invalid() {
        let _ = DestroyWindow(st.panel);
        st.panel = HWND::default();
    }
    // Explorer may still be starting up; the watchdog retries on the next tick.
    let tb = crate::taskbar::find_taskbar()?;
    st.tb = tb;
    let panel = build_panel(host, st)?;
    // The worker posts to the host, so it needs no restart; only the new panel
    // has to be painted once with the current reading.
    let _ = panel;
    Ok(())
}

/// The `TaskbarCreated` broadcast message id, registered once.
fn taskbar_created_message() -> u32 {
    use std::sync::OnceLock;
    static ID: OnceLock<u32> = OnceLock::new();
    *ID.get_or_init(|| unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) })
}

/// Puts the panel back on top, but only if something is actually covering it.
///
/// Explorer's taskbar is topmost too and re-raises itself on clicks in the bar,
/// which used to make the panel vanish until the next tick. Unconditionally
/// calling `SetWindowPos` every tick fixes that but makes the taskbar flicker
/// (each call re-orders the compositor). A hit test at the panel centre is a
/// cheap way to ask "am I still visible?" and only the rare covered case pays for
/// a re-order.
/// Shows or hides the panel to match the taskbar strip.
///
/// Returns `true` when the strip is on screen and the normal layout work should
/// run. Auto-hide moves the strip outside the monitor instead of hiding it, so
/// `IsWindowVisible` cannot detect it; the rect is compared against the monitor
/// the panel lives on.
///
/// Hidden state is tracked in the window itself (`IsWindowVisible`), so a repeat
/// call with an unchanged strip is free and cannot flicker the face.
unsafe fn follow_taskbar_visibility(hwnd: HWND, tb: &Taskbar) -> bool {
    let Ok(strip) = crate::taskbar::rect_in_screen(tb.rebar) else {
        return true; // Discovery is stale; leave the panel alone this tick.
    };
    let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
    let mut mi = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if !GetMonitorInfoW(monitor, &mut mi).as_bool() {
        return true;
    }
    let show = crate::taskbar::strip_visible(strip, mi.rcMonitor);
    let visible = IsWindowVisible(hwnd).as_bool();
    if show == visible {
        return show;
    }
    if show {
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        paint(hwnd);
    } else {
        hide_tip(hwnd);
        let _ = ShowWindow(hwnd, SW_HIDE);
    }
    show
}

/// Menu command ids.
const CMD_REFRESH: usize = 1;
const CMD_EXIT: usize = 2;
const CMD_AUTOSTART: usize = 3;

/// `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` value name.
const AUTOSTART_VALUE: PCWSTR = w!("RazerBatteryTaskbar");

/// Adds or removes this executable from the per-user Run key.
///
/// Per-user (`HKCU`) because the panel needs no elevation, and a single value
/// name so removal leaves no residue. The path is quoted: the binary may live
/// under a directory with spaces.
unsafe fn set_autostart(enable: bool) -> bool {
    use windows::Win32::System::Registry::{
        RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER,
        KEY_SET_VALUE, REG_SZ,
    };
    let mut key = HKEY::default();
    if RegOpenKeyExW(
        HKEY_CURRENT_USER,
        w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run"),
        None,
        KEY_SET_VALUE,
        &mut key,
    )
    .is_err()
    {
        return false;
    }
    let ok = if enable {
        let mut path = [0u16; 1024];
        let len = GetModuleFileNameW(None, &mut path);
        if len == 0 {
            false
        } else {
            let exe = String::from_utf16_lossy(&path[..len as usize]);
            let quoted = format!("\"{exe}\"");
            let wide: Vec<u16> = quoted.encode_utf16().chain(std::iter::once(0)).collect();
            RegSetValueExW(
                key,
                AUTOSTART_VALUE,
                None,
                REG_SZ,
                Some(std::slice::from_raw_parts(
                    wide.as_ptr() as *const u8,
                    wide.len() * 2,
                )),
            )
            .is_ok()
        }
    } else {
        RegDeleteValueW(key, AUTOSTART_VALUE).is_ok()
    };
    let _ = RegCloseKey(key);
    ok
}

/// Whether the Run key currently contains this value.
unsafe fn autostart_enabled() -> bool {
    use windows::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE,
    };
    let mut key = HKEY::default();
    if RegOpenKeyExW(
        HKEY_CURRENT_USER,
        w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run"),
        None,
        KEY_QUERY_VALUE,
        &mut key,
    )
    .is_err()
    {
        return false;
    }
    let mut buf = [0u8; 2048];
    let mut size = buf.len() as u32;
    let found = RegQueryValueExW(
        key,
        AUTOSTART_VALUE,
        None,
        None,
        Some(buf.as_mut_ptr()),
        Some(&mut size),
    )
    .is_ok();
    let _ = RegCloseKey(key);
    found
}

/// Shows the panel's context menu at the cursor: refresh now, or exit.
unsafe fn show_menu(hwnd: HWND) {
    let menu = CreatePopupMenu();
    let Ok(menu) = menu else {
        return;
    };
    let _ = AppendMenuW(menu, MF_STRING, CMD_REFRESH, w!("Refresh now"));
    let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
    // Checked when the Run key holds this executable, so the state is always
    // what the registry says rather than an in-memory guess.
    let autostart_flags = if autostart_enabled() {
        MF_STRING | MF_CHECKED
    } else {
        MF_STRING
    };
    let _ = AppendMenuW(
        menu,
        autostart_flags,
        CMD_AUTOSTART,
        w!("Start with Windows"),
    );
    let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
    let _ = AppendMenuW(menu, MF_STRING, CMD_EXIT, w!("Exit"));
    // Drop the menu *below* the taskbar instead of at the cursor: a menu that
    // overlaps the taskbar leaves its pixels there (explorer does not repaint
    // for a foreign topmost window), and cross-process RedrawWindow cannot fix
    // that reliably. Anchoring it outside the bar avoids the residue entirely.
    let mut pt = POINT::default();
    let mut panel_rc = RECT::default();
    if GetWindowRect(hwnd, &mut panel_rc).is_ok() {
        pt.x = panel_rc.left;
        pt.y = panel_rc.bottom;
    } else {
        let _ = GetCursorPos(&mut pt);
    }
    // TPM_RETURNCMD: the chosen id comes back from TrackPopupMenu instead of
    // arriving as a separate WM_COMMAND.
    let cmd = TrackPopupMenu(
        menu,
        TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_NONOTIFY,
        pt.x,
        pt.y,
        Some(0),
        hwnd,
        None,
    );
    let _ = DestroyMenu(menu);
    // The menu may still have touched the bar edge; repaint the panel and the
    // taskbar region it covers.
    paint(hwnd);
    let host = host_of(hwnd);
    if host.is_invalid() {
        return;
    }
    let st = &*state_ptr(host);
    let mut tray_rc = RECT::default();
    if GetWindowRect(st.tb.tray, &mut tray_rc).is_ok() {
        let flags = RDW_INVALIDATE | RDW_UPDATENOW | RDW_ALLCHILDREN | RDW_ERASE;
        let _ = RedrawWindow(Some(st.tb.tray), Some(&tray_rc), None, flags);
        let _ = RedrawWindow(Some(st.tb.rebar), None, None, flags);
    }
    match cmd.0 as usize {
        CMD_REFRESH => {
            if let Some(monitor) = (*state_ptr(host)).monitor.as_ref() {
                monitor.refresh();
            }
        }
        CMD_AUTOSTART => {
            let _ = set_autostart(!autostart_enabled());
        }
        CMD_EXIT => {
            // Tear down the host: it owns the timers, the monitor and the panel,
            // and `WM_DESTROY` restores the taskbar layout before quitting.
            let _ = DestroyWindow(host);
        }
        _ => {}
    }
}

/// Widens the panel if `label` needs more room than the current width allows.
///
/// The window is created snug for a two-digit reading; a three-digit reading
/// (100 %) needs one more digit and would otherwise be clipped. Only ever grows,
/// so the taskbar does not jitter as the percentage oscillates.
unsafe fn fit_to_label(hwnd: HWND, label: &str) {
    let host = host_of(hwnd);
    if host.is_invalid() {
        return;
    }
    let st = &mut *state_ptr(host);
    let mut rc = RECT::default();
    if GetClientRect(hwnd, &mut rc).is_err() {
        return;
    }
    let h = rc.bottom - rc.top;
    let needed = crate::taskbar::content_width(h, measure_label(label, h));
    if needed <= st.panel_w {
        return;
    }
    st.panel_w = needed;
    if crate::taskbar::embed(hwnd, &st.tb, needed).is_ok() {
        paint(hwnd);
    }
}

/// Paints the panel with a fresh window DC (timer path).
///
/// State lives on the host, so the panel forwards to it; the caller passes the
/// panel to draw and the host that owns the reading.
unsafe fn paint(hwnd: HWND) {
    let host = host_of(hwnd);
    if host.is_invalid() {
        return;
    }
    let mut rc = RECT::default();
    let _ = GetClientRect(hwnd, &mut rc);
    let w = rc.right - rc.left;
    let h = rc.bottom - rc.top;
    if w <= 0 || h <= 0 {
        return;
    }
    let snapshot = (*state_ptr(host))
        .monitor
        .as_ref()
        .map(crate::battery::BatteryMonitor::snapshot)
        .unwrap_or_default();
    // A three-digit reading needs more room than the snug initial width.
    fit_to_label(hwnd, &crate::battery::face_state(&snapshot).0);
    let hdc = GetDC(Some(hwnd));
    if hdc.is_invalid() {
        return;
    }
    let st = &*state_ptr(host);
    paint_into(hdc, w, h, st.text_color, &snapshot);
    let _ = ReleaseDC(Some(hwnd), hdc);
}

/// Background colour for the panel face, sampled once from the taskbar strip
/// just left of the panel.
///
/// A solid fill (rather than BitBlt from the screen) is mandatory: the panel is
/// topmost, so sampling the screen would copy its own previous frame back in
/// and ghost every label. Sampling happens outside the panel, once.
fn taskbar_background() -> COLORREF {
    use std::sync::OnceLock;
    static BG: OnceLock<COLORREF> = OnceLock::new();
    *BG.get_or_init(|| unsafe {
        const FALLBACK: COLORREF = COLORREF(0x00F2_F2F2);
        let screen = GetDC(None);
        if screen.is_invalid() {
            return FALLBACK;
        }
        let color = match panel_rect() {
            // 2 px left of the panel: taskbar surface, never the panel itself.
            Some(rc) => {
                let probe = GetPixel(screen, rc.left - 2, rc.top + 4);
                if probe.0 == CLR_INVALID {
                    FALLBACK
                } else {
                    probe
                }
            }
            None => FALLBACK,
        };
        let _ = ReleaseDC(None, screen);
        color
    })
}

/// Screen rect of this process's panel window, if it exists.
unsafe fn panel_rect() -> Option<RECT> {
    let hwnd = FindWindowW(PANEL_CLASS_NAME, w!("Razer Battery")).ok()?;
    if hwnd.is_invalid() {
        return None;
    }
    let mut rc = RECT::default();
    GetWindowRect(hwnd, &mut rc).ok()?;
    Some(rc)
}

unsafe fn paint_into(
    hdc: HDC,
    w: i32,
    h: i32,
    color: COLORREF,
    snapshot: &crate::battery::Snapshot,
) {
    let mem = CreateCompatibleDC(Some(hdc));
    let bmp = CreateCompatibleBitmap(hdc, w, h);
    if mem.is_invalid() || bmp.is_invalid() {
        if !bmp.is_invalid() {
            let _ = DeleteObject(bmp.into());
        }
        if !mem.is_invalid() {
            let _ = DeleteDC(mem);
        }
        return;
    }
    let old = SelectObject(mem, bmp.into());
    // Fill with the taskbar's own background colour instead of sampling the
    // screen. Sampling the screen reads the panel's *previous* frame (the panel
    // is topmost, so its own pixels are what is there), which fed old glyphs
    // back in as background and produced the smeared/ghosted text.
    let bg = taskbar_background();
    let full = RECT {
        left: 0,
        top: 0,
        right: w,
        bottom: h,
    };
    let bg_brush = CreateSolidBrush(bg);
    if !bg_brush.is_invalid() {
        let _ = FillRect(mem, &full, bg_brush);
        let _ = DeleteObject(bg_brush.into());
    }

    draw_face(mem, w, h, color, snapshot);

    let _ = BitBlt(hdc, 0, 0, w, h, Some(mem), 0, 0, SRCCOPY);
    SelectObject(mem, old);
    let _ = DeleteObject(bmp.into());
    let _ = DeleteDC(mem);
}

/// Draws battery glyph (left) and text (right) onto `hdc` of size `w`x`h`.
///
/// All geometry derives from the panel height so the face scales with DPI
/// (the panel is sized in physical pixels by [`crate::taskbar::embed`]).
///
/// State semantics (acceptance F1/F3): a failed read keeps the last valid
/// level but paints it dimmed with a stale marker; no reading at all paints
/// `--%`, never `0%`; charging is only shown when the mouse reports it.
unsafe fn draw_face(
    hdc: HDC,
    w: i32,
    h: i32,
    color: COLORREF,
    snapshot: &crate::battery::Snapshot,
) {
    use crate::battery::Availability;
    let stale = snapshot.availability != Availability::Ok;
    let color = if stale {
        // Dim ink for retained-but-stale or unknown values.
        COLORREF(0x00A0_A0A0)
    } else {
        color
    };
    let (label, fill) = crate::battery::face_state(snapshot);
    // Same geometry as `taskbar::glyph_width`: horizontal body + small nub.
    let pad = (h / 10).max(3);
    let gap = (h / 6).max(3);
    let margin = (h / 12).max(2);
    let bh = ((h - 2 * margin) * 7 / 10).max(4);
    let bw = (bh * 3 / 2).max(8);
    let nub_w = (bh / 6).clamp(1, 3);
    let bx = pad;
    let by = (h - bh) / 2;
    let text_x = bx + crate::taskbar::glyph_width(h) + gap;

    // Glyph outline with a hollow brush, solid pen sized with the panel.
    let pen_w = (h / 30).max(1);
    let pen = CreatePen(PS_SOLID, pen_w, color);
    if !pen.is_invalid() {
        let old_pen = SelectObject(hdc, pen.into());
        let old_brush = SelectObject(hdc, GetStockObject(NULL_BRUSH));
        let _ = Rectangle(hdc, bx, by, bx + bw, by + bh);
        // Terminal nub on the right edge.
        let nub_h = (bh / 2).max(4);
        let _ = Rectangle(
            hdc,
            bx + bw,
            by + (bh - nub_h) / 2,
            bx + bw + nub_w,
            by + (bh + nub_h) / 2,
        );
        SelectObject(hdc, old_brush);
        SelectObject(hdc, old_pen);
        let _ = DeleteObject(pen.into());
    }

    // Fill level inside the outline.
    let inset = pen_w + 1;
    let inner_w = (bw - 2 * inset).max(1);
    let fill_w = (inner_w as f32 * fill).round() as i32;
    let brush = CreateSolidBrush(color);
    if !brush.is_invalid() {
        let fill = RECT {
            left: bx + inset,
            top: by + inset,
            right: bx + inset + fill_w,
            bottom: by + bh - inset,
        };
        let _ = FillRect(hdc, &fill, brush);
        let _ = DeleteObject(brush.into());
    }

    // Charging bolt overlay (only when the mouse explicitly reports charging).
    if snapshot.charging == Some(true) {
        let bolt_w = (bw / 4).max(3);
        let bolt_h = (bh / 2).max(4);
        let cx = bx + bw / 2;
        let cy = by + bh / 2;
        let bolt = [
            POINT {
                x: cx,
                y: cy - bolt_h / 2,
            },
            POINT {
                x: cx - bolt_w / 2,
                y: cy,
            },
            POINT {
                x: cx + bolt_w / 4,
                y: cy,
            },
            POINT {
                x: cx - bolt_w / 4,
                y: cy + bolt_h / 2,
            },
        ];
        let bolt_brush = CreateSolidBrush(color);
        if !bolt_brush.is_invalid() {
            let old_bolt = SelectObject(hdc, bolt_brush.into());
            let _ = Polygon(hdc, &bolt);
            SelectObject(hdc, old_bolt);
            let _ = DeleteObject(bolt_brush.into());
        }
    }

    // Text fills the space right of the glyph, in a DPI-independent bold font
    // (stock DEFAULT_GUI_FONT stays tiny under high-DPI awareness).
    let mut text: Vec<u16> = label.encode_utf16().collect();
    let mut trc = RECT {
        left: text_x,
        top: 0,
        right: w - pad,
        bottom: h,
    };
    let font = CreateFontW(
        -(h * 2 / 5), // height ~40% of panel, negative = character height
        0,
        0,
        0,
        FW_SEMIBOLD.0 as i32,
        0,
        0,
        0,
        DEFAULT_CHARSET,
        FONT_OUTPUT_PRECISION(0),
        FONT_CLIP_PRECISION(0),
        CLEARTYPE_QUALITY,
        0,
        w!("Segoe UI"),
    );
    if !font.is_invalid() {
        let old_font = SelectObject(hdc, font.into());
        let _ = SetBkMode(hdc, TRANSPARENT);
        let prev = SetTextColor(hdc, color);
        // Left-aligned so truncation, if it ever happens again, hits the
        // right end instead of eating the leading characters.
        let _ = DrawTextW(
            hdc,
            &mut text,
            &mut trc,
            DT_LEFT | DT_SINGLELINE | DT_VCENTER,
        );
        SetTextColor(hdc, prev);
        SelectObject(hdc, old_font);
        let _ = DeleteObject(font.into());
    }
}

#[cfg(test)]
mod tests {
    use crate::battery::{face_state, Availability, Snapshot};

    #[test]
    fn no_reading_shows_placeholder_not_zero() {
        let (label, fill) = face_state(&Snapshot::default());
        assert_eq!(label, "--%");
        assert_eq!(fill, 0.0);
    }

    #[test]
    fn valid_zero_percent_is_a_reading() {
        let snapshot = Snapshot {
            availability: Availability::Ok,
            level: Some(0),
            ..Default::default()
        };
        let (label, fill) = face_state(&snapshot);
        assert_eq!(label, "0%"); // 0 % is legal, distinct from "no reading"
        assert_eq!(fill, 0.0);
    }

    #[test]
    fn stale_keeps_number_without_fill() {
        let snapshot = Snapshot {
            availability: Availability::Stale,
            level: Some(47),
            ..Default::default()
        };
        let (label, fill) = face_state(&snapshot);
        assert_eq!(label, "47%");
        assert_eq!(fill, 0.0);
    }
}

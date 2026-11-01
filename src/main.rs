//! Taskbar battery panel: HID polling, layout embedding and rendering.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod battery;
mod panel;
mod taskbar;

use windows::core::w;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, TranslateMessage, MSG,
};

/// Named mutex guarding the single-instance rule.
///
/// A second instance would reserve the strip a second time and shrink the
/// task-button area by another panel width, which looks like the taskbar
/// eating itself. The guard is process-wide and released by the OS on exit,
/// including a crash, so it can never go stale.
fn already_running() -> bool {
    unsafe {
        use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
        use windows::Win32::System::Threading::CreateMutexW;

        match CreateMutexW(
            None,
            false,
            w!("Global\\RazerBatteryTaskbar.SingleInstance"),
        ) {
            Ok(_handle) => {
                // The handle is intentionally leaked: it must live exactly as
                // long as the process, and the OS cleans it up on exit.
                GetLastError() == ERROR_ALREADY_EXISTS
            }
            // Cannot create a mutex (restricted session): do not block startup
            // on a guard that does not work.
            Err(_) => false,
        }
    }
}

fn fail(msg: String) -> ! {
    // Release builds have no console (windows_subsystem); the exit code still
    // tells launchers we refused to degrade to a floating window.
    if cfg!(debug_assertions) {
        eprintln!("error: {msg}");
    }
    std::process::exit(2);
}

fn main() {
    // Single instance: a second panel would reserve the strip again and shrink
    // the task-button area by another width.
    if already_running() {
        return;
    }

    // PerMonitorV2 must be set before any window is created. Best effort: on
    // failure the process keeps system-default awareness.
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }

    let tb = match taskbar::find_taskbar() {
        Ok(tb) => tb,
        Err(e) => fail(e),
    };
    // Panel width comes from the real content (glyph + label). It is measured
    // against a representative *two-digit* label, so the panel is snug for the
    // common case; `panel::create` widens it once if the first real reading
    // needs a third digit (see `fit_to_label`).
    let h = unsafe { taskbar::rect_in_screen(tb.mstask) }
        .map(|r| r.bottom - r.top)
        .unwrap_or(40);
    let text_w = unsafe { panel::measure_label(panel::TYPICAL_LABEL, h) };
    let panel_w = taskbar::content_width(h, text_w);
    let _panel: HWND = match panel::create(&tb, panel_w) {
        Ok(h) => h,
        Err(e) => fail(e),
    };

    let mut msg = MSG::default();
    loop {
        // BOOL: 0 = WM_QUIT, -1 = error, >0 = message dispatched.
        let r = unsafe { GetMessageW(&mut msg, None, 0, 0) }.0;
        if r == 0 {
            break;
        }
        if r == -1 {
            fail("GetMessageW failed".into());
        }
        #[cfg(debug_assertions)]
        {
            use std::io::Write;
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open("cp2a-msg.log")
            {
                let _ = writeln!(f, "msg 0x{:04X} hwnd={:?}", msg.message, msg.hwnd);
            }
        }
        unsafe {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

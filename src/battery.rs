//! Battery polling on a background thread and the state the panel renders.
//!
//! The worker owns the `razer-hid` handle and never touches the window: it
//! publishes snapshots through a shared slot and wakes the panel window with a
//! posted message. Every failure keeps the last valid reading and marks it
//! stale — a timeout or a missing device is never rendered as 0 %.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

/// Posted to the panel after each poll attempt (wakes the message loop).
pub const WM_BATTERY_UPDATED: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 1;

/// Delay between polls while the mouse answers.
const POLL_OK: Duration = Duration::from_secs(30);
/// Faster cadence while charging: the level moves and the user is watching.
const POLL_CHARGING: Duration = Duration::from_secs(10);
/// Back-off after a failed read (asleep, unplugged) so a sleeping mouse is not
/// kept awake by polling.
const POLL_FAILED: Duration = Duration::from_secs(60);

/// Availability of the reading shown in the panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Availability {
    /// Fresh reading from the mouse.
    Ok,
    /// Last valid reading retained; the latest attempt failed.
    Stale,
    /// No mouse was found by discovery.
    Disconnected,
}

/// One snapshot of what the panel should show.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub availability: Availability,
    /// `Some` only after a successful read (0 is a valid level).
    pub level: Option<u8>,
    /// `Some(true/false)` when the model reports charging; `None` = unknown/unsupported.
    pub charging: Option<bool>,
    pub model: Option<String>,
    /// When the last successful read happened.
    pub last_success: Option<Instant>,
}

impl Default for Snapshot {
    fn default() -> Self {
        Self {
            availability: Availability::Disconnected,
            level: None,
            charging: None,
            model: None,
            last_success: None,
        }
    }
}

/// Text for "no valid reading at all"; never rendered as `0%`.
pub const PLACEHOLDER_TEXT: &str = "--%";

/// The text and fill fraction a snapshot should render.
///
/// Shared by the painter and its tests, so the "never render a missing reading
/// as 0 %" and "stale keeps the number but drops the fill" rules cannot drift
/// apart from what is actually drawn.
pub fn face_state(snapshot: &Snapshot) -> (String, f32) {
    let fill = match (snapshot.availability, snapshot.level) {
        (Availability::Ok, Some(level)) => level as f32 / 100.0,
        _ => 0.0,
    };
    let label = match snapshot.level {
        Some(level) => format!("{level}%"),
        None => PLACEHOLDER_TEXT.to_owned(),
    };
    (label, fill)
}

/// Handle to the polling thread and its current snapshot.
pub struct BatteryMonitor {
    slot: Arc<Mutex<Snapshot>>,
    stop: Arc<AtomicBool>,
    refresh: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl BatteryMonitor {
    /// Starts the polling thread; `notify` receives [`WM_BATTERY_UPDATED`] after
    /// every attempt.
    pub fn start(notify: HWND) -> Self {
        let slot = Arc::new(Mutex::new(Snapshot::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let refresh = Arc::new(AtomicBool::new(false));
        let (worker_slot, worker_stop, worker_refresh) =
            (Arc::clone(&slot), Arc::clone(&stop), Arc::clone(&refresh));
        // HWND is a raw handle: pass it as an integer so the worker closure is
        // Send. PostMessageW is safe to call with it from any thread.
        let notify_raw = notify.0 as usize;
        let worker = thread::spawn(move || {
            run(
                worker_slot,
                worker_stop,
                worker_refresh,
                HWND(notify_raw as *mut _),
            );
        });
        Self {
            slot,
            stop,
            refresh,
            worker: Some(worker),
        }
    }

    /// Asks for an immediate poll (menu "refresh"); the worker coalesces it
    /// with its own schedule.
    pub fn refresh(&self) {
        self.refresh.store(true, Ordering::Relaxed);
    }

    /// Latest published snapshot (cloned; cheap).
    pub fn snapshot(&self) -> Snapshot {
        self.slot
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Asks the worker to stop and waits for it. The worker checks the flag at
    /// most once per poll interval; a sleeping mouse can delay this by one
    /// timeout, so callers should not block a critical UI path on it.
    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl Drop for BatteryMonitor {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Worker loop: poll, publish, sleep in short slices so shutdown stays
/// responsive.
fn run(slot: Arc<Mutex<Snapshot>>, stop: Arc<AtomicBool>, refresh: Arc<AtomicBool>, notify: HWND) {
    let hid = match razer_hid::Hid::new() {
        Ok(hid) => hid,
        Err(_) => {
            // HID subsystem unavailable: report disconnected and stop.
            let mut guard = slot.lock().unwrap_or_else(PoisonError::into_inner);
            guard.availability = Availability::Disconnected;
            guard.level = None;
            guard.charging = None;
            guard.last_success = None;
            let _ = unsafe {
                PostMessageW(
                    Some(notify),
                    WM_BATTERY_UPDATED,
                    Default::default(),
                    Default::default(),
                )
            };
            return;
        }
    };

    while !stop.load(Ordering::Relaxed) {
        let delay = poll_once(&hid, &slot);
        notify_window(notify);
        sleep_unless_stopped(&stop, &refresh, delay);
    }
}

/// One poll: discover, read, publish. Returns how long to wait before the next
/// attempt.
///
/// All HID I/O happens *before* the snapshot lock is taken, so the UI thread's
/// `snapshot()` never waits on a device that is timing out or retrying.
fn poll_once(hid: &razer_hid::Hid, slot: &Mutex<Snapshot>) -> Duration {
    let outcome = read(hid);
    let next = match &outcome {
        Outcome::Reading { charging, .. } => {
            if charging == &Some(true) {
                POLL_CHARGING
            } else {
                POLL_OK
            }
        }
        // A sleeping or missing mouse must not be polled at the healthy
        // cadence: back off so we never keep the device awake (and never
        // hammer a dongle that has nothing attached).
        Outcome::NoDevice | Outcome::Failed => POLL_FAILED,
    };
    {
        let mut guard = slot.lock().unwrap_or_else(PoisonError::into_inner);
        publish(&mut guard, outcome, Instant::now());
    }
    next
}

/// What one read attempt produced, before it is published.
enum Outcome {
    Reading {
        level: u8,
        charging: Option<bool>,
        model: String,
    },
    NoDevice,
    Failed,
}

/// Does the device discovery and battery query. Never touches the snapshot.
fn read(hid: &razer_hid::Hid) -> Outcome {
    let devices = match hid.devices() {
        Ok(devices) => devices,
        Err(_) => return Outcome::NoDevice,
    };
    let Some(device) = devices.first() else {
        return Outcome::NoDevice;
    };
    match device.battery() {
        Ok(battery) => Outcome::Reading {
            level: battery.level.get(),
            charging: battery.charging,
            model: device.model().to_owned(),
        },
        Err(_) => Outcome::Failed,
    }
}

/// Applies one read attempt to a snapshot without any I/O.
///
/// Kept as a free function so the F1/F2/F3 state transitions are testable
/// without a device: a failed read keeps the last valid level but drops the
/// charging claim; a missing device drops the level entirely (the mouse it
/// belonged to is gone).
fn publish(snapshot: &mut Snapshot, outcome: Outcome, now: Instant) {
    match outcome {
        Outcome::Reading {
            level,
            charging,
            model,
        } => {
            snapshot.availability = Availability::Ok;
            snapshot.level = Some(level);
            snapshot.charging = charging;
            snapshot.model = Some(model);
            snapshot.last_success = Some(now);
        }
        Outcome::NoDevice => {
            snapshot.availability = Availability::Disconnected;
            snapshot.level = None;
            snapshot.charging = None;
        }
        Outcome::Failed => {
            snapshot.availability = Availability::Stale;
            snapshot.charging = None; // unknown while failing, never "not charging"
        }
    }
}

fn notify_window(hwnd: HWND) {
    let _ = unsafe {
        PostMessageW(
            Some(hwnd),
            WM_BATTERY_UPDATED,
            Default::default(),
            Default::default(),
        )
    };
}

/// Sleeps in slices so a stop or refresh request does not wait a full poll
/// interval. A refresh is consumed here, which makes back-to-back menu clicks
/// coalesce into one extra poll.
fn sleep_unless_stopped(stop: &AtomicBool, refresh: &AtomicBool, total: Duration) {
    let slice = Duration::from_millis(200);
    let mut slept = Duration::ZERO;
    while slept < total && !stop.load(Ordering::Relaxed) {
        if refresh.swap(false, Ordering::Relaxed) {
            return;
        }
        thread::sleep(slice.min(total - slept));
        slept += slice;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot_with_reading() -> Snapshot {
        Snapshot {
            availability: Availability::Ok,
            level: Some(47),
            charging: Some(false),
            model: Some("Razer DeathAdder V3 Pro".into()),
            last_success: Some(Instant::now()),
        }
    }

    #[test]
    fn failed_read_keeps_level_but_drops_charging() {
        let mut snapshot = snapshot_with_reading();
        publish(&mut snapshot, Outcome::Failed, Instant::now());
        assert_eq!(snapshot.level, Some(47)); // last valid reading kept
        assert_eq!(snapshot.charging, None); // unknown, not "not charging"
        assert_eq!(snapshot.availability, Availability::Stale);
    }

    #[test]
    fn disconnected_drops_the_stale_level() {
        // F2: the mouse is gone, so its old percentage must not be shown.
        let mut snapshot = snapshot_with_reading();
        publish(&mut snapshot, Outcome::NoDevice, Instant::now());
        assert_eq!(snapshot.level, None);
        assert_eq!(snapshot.charging, None);
        assert_eq!(snapshot.availability, Availability::Disconnected);
    }

    #[test]
    fn reading_publishes_values() {
        let mut snapshot = Snapshot::default();
        publish(
            &mut snapshot,
            Outcome::Reading {
                level: 0,
                charging: Some(true),
                model: "m".into(),
            },
            Instant::now(),
        );
        assert_eq!(snapshot.availability, Availability::Ok);
        assert_eq!(snapshot.level, Some(0)); // 0 % is a legal reading
        assert_eq!(snapshot.charging, Some(true));
    }

    #[test]
    fn default_snapshot_shows_nothing() {
        let snapshot = Snapshot::default();
        assert_eq!(snapshot.level, None);
        assert_eq!(snapshot.availability, Availability::Disconnected);
    }

    #[test]
    fn refresh_breaks_the_sleep_early() {
        let stop = AtomicBool::new(false);
        let refresh = AtomicBool::new(true);
        let started = Instant::now();
        // Would sleep a full minute without the refresh flag.
        sleep_unless_stopped(&stop, &refresh, Duration::from_secs(60));
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(!refresh.load(Ordering::Relaxed)); // consumed by the loop
    }

    #[test]
    fn stop_breaks_the_sleep_early() {
        let stop = AtomicBool::new(true);
        let refresh = AtomicBool::new(false);
        let started = Instant::now();
        sleep_unless_stopped(&stop, &refresh, Duration::from_secs(60));
        assert!(started.elapsed() < Duration::from_secs(1));
    }
}

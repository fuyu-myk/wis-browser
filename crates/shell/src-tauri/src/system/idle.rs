//! System-wide time since the last keyboard or pointer input.

use std::time::Duration;

#[cfg(target_os = "macos")]
pub fn since_input() -> Option<Duration> {
    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGEventSourceSecondsSinceLastEventType(state: i32, event_type: u32) -> f64;
    }
    const COMBINED_SESSION_STATE: i32 = 0;
    const ANY_INPUT_EVENT: u32 = u32::MAX;

    // SAFETY: a pure query taking two integers; callable from any thread.
    let secs =
        unsafe { CGEventSourceSecondsSinceLastEventType(COMBINED_SESSION_STATE, ANY_INPUT_EVENT) };
    Some(Duration::try_from_secs_f64(secs).unwrap_or(Duration::MAX))
}

#[cfg(windows)]
pub fn since_input() -> Option<Duration> {
    use windows::Win32::System::SystemInformation::GetTickCount;
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};

    let mut info = LASTINPUTINFO {
        cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
        dwTime: 0,
    };
    // SAFETY: `info` is a correctly sized, writable LASTINPUTINFO.
    if !unsafe { GetLastInputInfo(&mut info) }.as_bool() {
        return Some(Duration::MAX);
    }
    // SAFETY: no arguments; reads the system tick counter.
    let now = unsafe { GetTickCount() };
    Some(Duration::from_millis(u64::from(
        now.wrapping_sub(info.dwTime),
    )))
}

/// No portable idle source here yet, so usage counts focused time without a
/// cutoff.
#[cfg(not(any(target_os = "macos", windows)))]
pub fn since_input() -> Option<Duration> {
    None
}

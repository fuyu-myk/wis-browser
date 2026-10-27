//! Spoken announcements for assistive tech.
//!
//! The chrome asks VoiceOver to speak toasts and other status messages directly
//! (`NSAccessibilityAnnouncementRequestedNotification`). Elsewhere the page's
//! own live region does the work, so the command does nothing.

use wis_core::WisResult;

const MAX_CHARS: usize = 300;

/// The text as spoken: control characters and whitespace runs become single
/// spaces, invisible format characters (zero-width, bidi overrides) go, and
/// the result is capped, or `None` when nothing is left.
fn spoken(text: &str) -> Option<String> {
    let visible: String = text
        .chars()
        .filter(|c| !matches!(c, '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}'))
        .collect();
    let collapsed = visible
        .split(|c: char| c.is_whitespace() || c.is_control())
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let capped: String = collapsed.chars().take(MAX_CHARS).collect();
    (!capped.is_empty()).then_some(capped)
}

/// Ask the screen reader to speak `text`; `urgent` interrupts current speech.
#[tauri::command]
pub async fn announce(app: tauri::AppHandle, text: String, urgent: bool) -> WisResult<()> {
    let Some(text) = spoken(&text) else {
        return Ok(());
    };
    #[cfg(target_os = "macos")]
    {
        let queued = app.run_on_main_thread(move || post(&text, urgent));
        if let Err(err) = queued {
            log::warn!("announcement not queued: {err}");
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (app, text, urgent);
    Ok(())
}

#[cfg(target_os = "macos")]
fn post(text: &str, urgent: bool) {
    use objc2_06::runtime::AnyObject;
    use objc2_06::MainThreadMarker;
    use objc2_app_kit::{
        NSAccessibilityAnnouncementKey, NSAccessibilityAnnouncementRequestedNotification,
        NSAccessibilityPostNotificationWithUserInfo, NSAccessibilityPriorityKey,
        NSAccessibilityPriorityLevel, NSApplication,
    };
    use objc2_foundation::{NSDictionary, NSNumber, NSString};

    let Some(mtm) = MainThreadMarker::new() else {
        log::warn!("announcement posted off the main thread");
        return;
    };
    let app = NSApplication::sharedApplication(mtm);
    let priority = if urgent {
        NSAccessibilityPriorityLevel::High
    } else {
        NSAccessibilityPriorityLevel::Medium
    };
    let message = NSString::from_str(text);
    let level = NSNumber::new_isize(priority.0);
    // SAFETY: AppKit's own constants; the statics live for the process.
    let (keys, notification) = unsafe {
        (
            [NSAccessibilityAnnouncementKey, NSAccessibilityPriorityKey],
            NSAccessibilityAnnouncementRequestedNotification,
        )
    };
    let info =
        NSDictionary::<_, AnyObject>::from_slices(&keys, &[&*message as &AnyObject, &*level]);
    // SAFETY: main thread; the element is the application and the user info
    // carries the documented key types (NSString text, NSNumber priority).
    unsafe { NSAccessibilityPostNotificationWithUserInfo(&app, notification, Some(&info)) };
}

#[cfg(test)]
mod tests {
    use super::spoken;

    #[test]
    fn whitespace_and_control_characters_collapse_to_single_spaces() {
        assert_eq!(
            spoken("  Download\n\tcomplete:\u{7} file.pdf  ").as_deref(),
            Some("Download complete: file.pdf")
        );
    }

    #[test]
    fn empty_or_blank_text_is_not_spoken() {
        assert_eq!(spoken(""), None);
        assert_eq!(spoken(" \n\u{0} "), None);
    }

    #[test]
    fn invisible_format_characters_are_not_spoken() {
        assert_eq!(
            spoken("safe\u{202E}txt.exe\u{200B}").as_deref(),
            Some("safetxt.exe")
        );
    }

    #[test]
    fn long_text_is_capped_by_characters_not_bytes() {
        let text = "é".repeat(400);
        assert_eq!(spoken(&text).map(|s| s.chars().count()), Some(300));
    }
}

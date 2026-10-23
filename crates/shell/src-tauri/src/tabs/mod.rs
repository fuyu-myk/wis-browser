//! Tab lifecycle: sessions, archiving, previews, load state, media and back and forward.

pub(crate) mod archive;
pub(crate) mod audio;
pub(crate) mod autoplay;
pub(crate) mod interaction_state;
#[cfg(any(windows, target_os = "linux"))]
pub(crate) mod load_failure;
pub(crate) mod load_progress;
pub(crate) mod preview;
pub(crate) mod session;
pub(crate) mod swipe_nav;
pub(crate) mod zoom;

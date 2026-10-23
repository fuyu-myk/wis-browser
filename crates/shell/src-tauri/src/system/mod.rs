//! OS services and dialogs: the default browser, other apps, the clipboard, files, idle and memory.

pub(crate) mod app_launch;
pub(crate) mod clipboard;
pub(crate) mod default_browser;
pub(crate) mod external;
pub(crate) mod fatal_dialog;
pub(crate) mod idle;
pub(crate) mod memory;
pub(crate) mod panels;
pub(crate) mod wake;
#[cfg(windows)]
pub(crate) mod win_default_browser;

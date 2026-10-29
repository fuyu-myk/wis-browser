//! On-device page translation through Apple's Translation framework (macOS
//! 26.4 and later), reached through the Swift bridge in
//! `swift/Translation.swift`.

mod bridge;

pub use bridge::{translate, Run, Strategy, TranslateError, TranslateRequest};

//! The Rust side of the Swift bridge: one block per call, answered through a
//! C callback into a oneshot channel.
//!
//! The framework does its work on the main run loop. Callers are tokio tasks.

use std::ffi::{c_char, c_void, CStr, CString};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;

/// Longest one block may take.
const REPLY_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Strategy {
    /// Fluent, keeps inline elements on the right words; about 0.7 s a block.
    HighFidelity,
    /// About 0.05 s a block, but inline elements are lost; needs a download.
    LowLatency,
}

/// A piece of a block's text, tagged with the inline element it sits in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Run {
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct TranslateRequest<'a> {
    pub source: &'a str,
    pub target: &'a str,
    pub strategy: Strategy,
    pub runs: &'a [Run],
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TranslateError {
    #[error("translation needs macOS 26.4 or later")]
    Unavailable,
    #[error("the language pair is not installed")]
    NotInstalled,
    #[error("the language pair is not supported")]
    Unsupported,
    #[error("nothing to translate")]
    NothingToTranslate,
    #[error("translation timed out")]
    TimedOut,
    #[error("translation failed: {0}")]
    Failed(String),
}

#[derive(Deserialize)]
struct Reply {
    runs: Option<Vec<Run>>,
    error: Option<String>,
}

type ReplyFn = extern "C" fn(*mut c_void, *const c_char);

extern "C" {
    fn wis_translation_translate(request: *const c_char, context: *mut c_void, reply: ReplyFn);
}

/// Called by the bridge exactly once per request, on a Swift executor thread.
extern "C" fn on_reply(context: *mut c_void, reply: *const c_char) {
    // SAFETY: `context` is the sender `translate` leaked for this request, and
    // the bridge answers each request once.
    let sender = unsafe { Box::from_raw(context.cast::<oneshot::Sender<String>>()) };
    // SAFETY: the bridge passes a NUL-terminated string that outlives the call.
    let reply = unsafe { CStr::from_ptr(reply) }
        .to_string_lossy()
        .into_owned();
    if sender.send(reply).is_err() {
        log::debug!("translation reply arrived after its caller gave up");
    }
}

/// Translates one block. Must not be awaited on the main thread.
pub async fn translate(request: &TranslateRequest<'_>) -> Result<Vec<Run>, TranslateError> {
    // SAFETY: `pthread_main_np` has no preconditions.
    debug_assert!(
        unsafe { libc::pthread_main_np() } == 0,
        "a translation awaited on the main thread never finishes"
    );
    let json =
        serde_json::to_string(request).map_err(|err| TranslateError::Failed(err.to_string()))?;
    let json = CString::new(json).expect("serde_json escapes NUL");
    let (sender, receiver) = oneshot::channel::<String>();
    let context = Box::into_raw(Box::new(sender)).cast::<c_void>();
    // SAFETY: the bridge copies `json` before returning and hands `context`
    // back to `on_reply` exactly once, which reclaims it.
    unsafe { wis_translation_translate(json.as_ptr(), context, on_reply) };
    let reply = tokio::time::timeout(REPLY_TIMEOUT, receiver)
        .await
        .map_err(|_| TranslateError::TimedOut)?
        .map_err(|_| TranslateError::Failed("the bridge dropped its reply".into()))?;
    parse_reply(&reply)
}

fn parse_reply(reply: &str) -> Result<Vec<Run>, TranslateError> {
    let reply: Reply =
        serde_json::from_str(reply).map_err(|err| TranslateError::Failed(err.to_string()))?;
    match (reply.runs, reply.error.as_deref()) {
        (Some(runs), None) => Ok(runs),
        (_, Some("unavailable")) => Err(TranslateError::Unavailable),
        (_, Some("notInstalled")) => Err(TranslateError::NotInstalled),
        (
            _,
            Some(
                "unsupportedSourceLanguage"
                | "unsupportedTargetLanguage"
                | "unsupportedLanguagePairing",
            ),
        ) => Err(TranslateError::Unsupported),
        (_, Some("nothingToTranslate")) => Err(TranslateError::NothingToTranslate),
        (_, Some(other)) => Err(TranslateError::Failed(other.to_string())),
        (None, None) => Err(TranslateError::Failed("an empty reply".into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(text: &str, tag: Option<u32>) -> Run {
        Run {
            text: text.into(),
            tag,
        }
    }

    #[test]
    fn a_reply_with_runs_is_the_translation() {
        let runs = parse_reply(r#"{"runs":[{"text":"Read "},{"text":"this","tag":1}]}"#).unwrap();
        assert_eq!(runs, vec![run("Read ", None), run("this", Some(1))]);
    }

    #[test]
    fn bridge_error_codes_map_to_errors() {
        let cases = [
            ("unavailable", TranslateError::Unavailable),
            ("notInstalled", TranslateError::NotInstalled),
            ("unsupportedSourceLanguage", TranslateError::Unsupported),
            ("unsupportedTargetLanguage", TranslateError::Unsupported),
            ("unsupportedLanguagePairing", TranslateError::Unsupported),
            ("nothingToTranslate", TranslateError::NothingToTranslate),
            ("cancelled", TranslateError::Failed("cancelled".into())),
        ];
        for (code, expected) in cases {
            let reply = format!(r#"{{"error":"{code}"}}"#);
            assert_eq!(parse_reply(&reply), Err(expected), "{code}");
        }
    }

    #[test]
    fn a_malformed_reply_is_a_failure() {
        assert!(matches!(
            parse_reply("not json"),
            Err(TranslateError::Failed(_))
        ));
        assert!(matches!(parse_reply("{}"), Err(TranslateError::Failed(_))));
    }

    #[test]
    fn requests_use_the_bridge_field_names() {
        let runs = [run("こんにちは", None), run("世界", Some(2))];
        let request = TranslateRequest {
            source: "ja",
            target: "en",
            strategy: Strategy::LowLatency,
            runs: &runs,
        };
        assert_eq!(
            serde_json::to_string(&request).unwrap(),
            r#"{"source":"ja","target":"en","strategy":"lowLatency","runs":[{"text":"こんにちは"},{"text":"世界","tag":2}]}"#
        );
    }

    /// A real round trip through the linked Swift library that stops before
    /// the framework, so it runs without models or a main run loop.
    #[tokio::test]
    async fn a_blank_block_round_trips_through_the_bridge() {
        let runs = [run("  \n ", None)];
        let request = TranslateRequest {
            source: "ja",
            target: "en",
            strategy: Strategy::HighFidelity,
            runs: &runs,
        };
        assert_eq!(
            translate(&request).await,
            Err(TranslateError::NothingToTranslate)
        );
    }
}

//! The Rust side of the Swift bridge: one block per call, answered through a
//! C callback into a oneshot channel.
//!
//! The framework does its work on the main run loop. Callers are tokio tasks.

use std::ffi::{c_char, c_void, CStr, CString};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;

use super::message::Run;

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

#[derive(Debug, Serialize)]
pub struct TranslateRequest<'a> {
    pub source: &'a str,
    pub target: &'a str,
    pub strategy: Strategy,
    pub runs: &'a [Run],
    /// Tags whose text the framework is told not to translate.
    #[serde(skip_serializing_if = "<[u32]>::is_empty")]
    pub skip: &'a [u32],
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TranslateError {
    #[error("translation needs macOS 26.4 or later")]
    Unavailable,
    #[error("the page's language is unknown")]
    NoLanguage,
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

/// Whether a language pair can be translated with one strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Status {
    Installed,
    /// Supported, but its models need a download.
    Supported,
    Unsupported,
}

/// What translating a page from one language involves: the target (the first
/// macOS preferred language) and what each strategy can do for the pair.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Check {
    pub target: String,
    pub source_name: String,
    pub target_name: String,
    /// The page is already in the target language, regional variants aside.
    pub same_language: bool,
    pub high_fidelity: Status,
    pub low_latency: Status,
}

#[derive(Deserialize)]
struct Reply {
    runs: Option<Vec<Run>>,
    error: Option<String>,
}

#[derive(Serialize)]
struct CheckRequest<'a> {
    source: &'a str,
}

type ReplyFn = extern "C" fn(*mut c_void, *const c_char);
type EntryFn = unsafe extern "C" fn(*const c_char, *mut c_void, ReplyFn);

extern "C" {
    fn wis_translation_translate(request: *const c_char, context: *mut c_void, reply: ReplyFn);
    fn wis_translation_check(request: *const c_char, context: *mut c_void, reply: ReplyFn);
}

/// Called by the bridge exactly once per request, on a Swift executor thread.
extern "C" fn on_reply(context: *mut c_void, reply: *const c_char) {
    // SAFETY: `context` is the sender `call` leaked for this request, and the
    // bridge answers each request once.
    let sender = unsafe { Box::from_raw(context.cast::<oneshot::Sender<String>>()) };
    // SAFETY: the bridge passes a NUL-terminated string that outlives the call.
    let reply = unsafe { CStr::from_ptr(reply) }
        .to_string_lossy()
        .into_owned();
    if sender.send(reply).is_err() {
        log::debug!("translation reply arrived after its caller gave up");
    }
}

/// Sends `request` to a bridge entry point and waits for its JSON reply.
async fn call(entry: EntryFn, request: &impl Serialize) -> Result<String, TranslateError> {
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
    unsafe { entry(json.as_ptr(), context, on_reply) };
    tokio::time::timeout(REPLY_TIMEOUT, receiver)
        .await
        .map_err(|_| TranslateError::TimedOut)?
        .map_err(|_| TranslateError::Failed("the bridge dropped its reply".into()))
}

/// Translates one block. Must not be awaited on the main thread.
pub async fn translate(request: &TranslateRequest<'_>) -> Result<Vec<Run>, TranslateError> {
    parse_reply(&call(wis_translation_translate, request).await?)
}

/// What translating a page in `source` (a BCP 47 tag) involves. Must not be
/// awaited on the main thread.
pub async fn check(source: &str) -> Result<Check, TranslateError> {
    parse_check_reply(&call(wis_translation_check, &CheckRequest { source }).await?)
}

fn error_from_code(code: &str) -> TranslateError {
    match code {
        "unavailable" => TranslateError::Unavailable,
        "noLanguage" => TranslateError::NoLanguage,
        "notInstalled" => TranslateError::NotInstalled,
        "unsupportedSourceLanguage"
        | "unsupportedTargetLanguage"
        | "unsupportedLanguagePairing" => TranslateError::Unsupported,
        "nothingToTranslate" => TranslateError::NothingToTranslate,
        other => TranslateError::Failed(other.to_string()),
    }
}

fn parse_reply(reply: &str) -> Result<Vec<Run>, TranslateError> {
    let reply: Reply =
        serde_json::from_str(reply).map_err(|err| TranslateError::Failed(err.to_string()))?;
    match (reply.runs, reply.error.as_deref()) {
        (Some(runs), None) => Ok(runs),
        (_, Some(code)) => Err(error_from_code(code)),
        (None, None) => Err(TranslateError::Failed("an empty reply".into())),
    }
}

fn parse_check_reply(reply: &str) -> Result<Check, TranslateError> {
    let value: serde_json::Value =
        serde_json::from_str(reply).map_err(|err| TranslateError::Failed(err.to_string()))?;
    if let Some(code) = value.get("error").and_then(serde_json::Value::as_str) {
        return Err(error_from_code(code));
    }
    serde_json::from_value(value).map_err(|err| TranslateError::Failed(err.to_string()))
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
            skip: &[],
        };
        assert_eq!(
            serde_json::to_string(&request).unwrap(),
            r#"{"source":"ja","target":"en","strategy":"lowLatency","runs":[{"text":"こんにちは"},{"text":"世界","tag":2}]}"#
        );
        let held = TranslateRequest {
            skip: &[2],
            ..request
        };
        assert!(serde_json::to_string(&held)
            .unwrap()
            .ends_with(r#""tag":2}],"skip":[2]}"#));
    }

    #[test]
    fn a_check_reply_reads_the_target_names_and_statuses() {
        let check = parse_check_reply(
            r#"{"target":"en","sourceName":"Japanese","targetName":"English","sameLanguage":false,"highFidelity":"installed","lowLatency":"supported"}"#,
        )
        .unwrap();
        assert_eq!(
            check,
            Check {
                target: "en".into(),
                source_name: "Japanese".into(),
                target_name: "English".into(),
                same_language: false,
                high_fidelity: Status::Installed,
                low_latency: Status::Supported,
            }
        );
    }

    #[test]
    fn a_check_error_maps_like_a_translation_error() {
        assert_eq!(
            parse_check_reply(r#"{"error":"unavailable"}"#),
            Err(TranslateError::Unavailable)
        );
        assert_eq!(
            parse_check_reply(r#"{"error":"noLanguage"}"#),
            Err(TranslateError::NoLanguage)
        );
        assert!(matches!(
            parse_check_reply(r#"{"target":"en"}"#),
            Err(TranslateError::Failed(_))
        ));
    }

    #[tokio::test]
    async fn a_blank_language_round_trips_through_the_bridge() {
        assert_eq!(check(" ").await, Err(TranslateError::NoLanguage));
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
            skip: &[],
        };
        assert_eq!(
            translate(&request).await,
            Err(TranslateError::NothingToTranslate)
        );
    }
}

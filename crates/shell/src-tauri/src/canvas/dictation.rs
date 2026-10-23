//! Spoken prompts for the canvas agent. The microphone feeds Apple's speech
//! recognition, required to run on device so audio never leaves the Mac, and
//! what it hears streams to the canvas page as `window.__wisCanvas.mic(...)`.
//! One dictation runs at a time; it ends when the page stops it, the tab goes,
//! or after `MAX_LENGTH`.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Manager};
use wis_core::{WisError, WisResult};

/// Whether this platform can dictate.
pub const SUPPORTED: bool = cfg!(target_os = "macos");
const MAX_LENGTH: Duration = Duration::from_secs(120);
/// How long a stopped dictation may take to hand over its last words.
const SETTLE: Duration = Duration::from_secs(3);

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum MicEvent {
    Heard { text: String, done: bool },
    Failed { message: String },
    Ended,
}

static GENERATION: AtomicU64 = AtomicU64::new(0);

/// The tab dictating, claimed before any permission is asked so a stop, a
/// reload or a closed tab during the asking is never missed. Read off the
/// main thread.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Owner {
    label: String,
    generation: u64,
    stopping: bool,
}

static OWNER: Mutex<Option<Owner>> = Mutex::new(None);

fn refused(message: &str) -> WisError {
    WisError::InvalidInput {
        message: message.into(),
    }
}

fn owner() -> Option<Owner> {
    OWNER.lock().ok().and_then(|o| o.clone())
}

/// Claims the microphone for a tab: a new generation, none when the tab
/// already has it, or why not.
fn claim(label: &str) -> Result<Option<u64>, &'static str> {
    let mut slot = OWNER.lock().map_err(|_| "dictation could not start")?;
    match slot.as_ref() {
        Some(o) if o.label == label && o.stopping => Err("dictation is still finishing; try again"),
        Some(o) if o.label == label => Ok(None),
        Some(_) => Err("dictation is already on in another canvas"),
        None => {
            let generation = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
            *slot = Some(Owner {
                label: label.to_string(),
                generation,
                stopping: false,
            });
            Ok(Some(generation))
        }
    }
}

/// Whether `generation` may still open the microphone for `label`.
fn still_wanted(label: &str, generation: u64) -> bool {
    owner().is_some_and(|o| o.label == label && o.generation == generation && !o.stopping)
}

/// Marks the tab's dictation as ending; its generation, if it had one.
fn ending(label: &str) -> Option<u64> {
    let mut slot = OWNER.lock().ok()?;
    let owner = slot.as_mut().filter(|o| o.label == label)?;
    owner.stopping = true;
    Some(owner.generation)
}

fn release_owner(generation: u64) {
    if let Ok(mut o) = OWNER.lock() {
        if o.as_ref().is_some_and(|o| o.generation == generation) {
            *o = None;
        }
    }
}

/// Hands an event to the tab, if it is still on the canvas; false if not.
fn push(app: &AppHandle, label: &str, event: &MicEvent) -> bool {
    let Some(webview) = app.get_webview(label) else {
        return false;
    };
    if !webview
        .url()
        .is_ok_and(|url| crate::canvas::page::is_canvas_url(&url))
    {
        return false;
    }

    let Ok(json) = serde_json::to_string(event) else {
        return false;
    };
    webview
        .eval(format!(
            "window.__wisCanvas&&window.__wisCanvas.mic({json})"
        ))
        .is_ok()
}

/// Starts dictating for a canvas tab, asking for the speech and microphone
/// permissions the first time.
pub async fn start(app: &AppHandle, label: &str) -> WisResult<()> {
    let Some(generation) = claim(label).map_err(refused)? else {
        return Ok(());
    };
    let begun = open(app, label, generation).await;
    if begun.is_err() {
        release_owner(generation);
        return begun;
    }

    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(MAX_LENGTH).await;
        stop_generation(&app, generation);
    });

    Ok(())
}

async fn open(app: &AppHandle, label: &str, generation: u64) -> WisResult<()> {
    native::authorize().await.map_err(|why| refused(&why))?;
    let (tx, rx) = tokio::sync::oneshot::channel();
    let (main_app, tab) = (app.clone(), label.to_string());
    app.run_on_main_thread(move || {
        let allowed = still_wanted(&tab, generation) && crate::canvas::page::is_canvas_tab(&tab);
        let _ = tx.send(if allowed {
            native::begin(main_app, tab, generation)
        } else {
            Err("dictation was stopped")
        });
    })
    .map_err(|_| refused("dictation could not start"))?;
    rx.await
        .map_err(|_| refused("dictation could not start"))?
        .map_err(refused)
}

/// Stops listening; the last words arrive, then `Ended`.
pub fn stop(app: &AppHandle, label: &str) {
    if let Some(generation) = ending(label) {
        stop_generation(app, generation);
    }
}

fn stop_generation(app: &AppHandle, generation: u64) {
    let main = app.clone();
    let _ = app.run_on_main_thread(move || native::stop(&main, generation));
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(SETTLE).await;
        let _ = app
            .clone()
            .run_on_main_thread(move || native::end(&app, generation, true));
    });
}

/// Called when a tab or its window closes: its dictation ends at once.
pub fn forget(app: &AppHandle, label: &str) {
    if let Some(generation) = ending(label) {
        let main = app.clone();
        let _ = app.run_on_main_thread(move || native::end(&main, generation, true));
    }
}

/// Why dictation cannot ask for permission: macOS checks the app that started
/// Wis.
fn cannot_ask(host: Option<&str>) -> String {
    match host {
        Some(name) => format!(
            "dictation cannot ask for permission while Wis runs inside {name}; open Wis as an app to use it"
        ),
        None => "dictation cannot ask for permission from here; open Wis as an app to use it".into(),
    }
}

/// What a recognition error means for the user, by its code; None when it
/// only says the recognition ended or was cancelled.
fn failure(code: isize, stopping: bool) -> Option<&'static str> {
    const NO_SPEECH: isize = 1110;
    const CANCELLED: [isize; 2] = [203, 216];
    if stopping || CANCELLED.contains(&code) {
        return None;
    }
    Some(if code == NO_SPEECH {
        "Didn't hear anything."
    } else {
        "Dictation stopped."
    })
}

#[cfg(target_os = "macos")]
mod native {
    use std::cell::RefCell;
    use std::ffi::{c_char, CStr, CString};

    use block2::RcBlock;
    use objc2::rc::autoreleasepool;
    use objc2::runtime::{AnyObject, Bool};
    use objc2::{class, msg_send, sel};
    use tauri::AppHandle;

    use super::{failure, push, release_owner, MicEvent};

    #[link(name = "Speech", kind = "framework")]
    extern "C" {}

    #[link(name = "AVFoundation", kind = "framework")]
    extern "C" {
        static AVMediaTypeAudio: *const AnyObject;
    }

    const AUTHORIZED: isize = 3;
    const NOT_DETERMINED: isize = 0;
    const TAP_FRAMES: u32 = 1024;

    type Obj = *mut AnyObject;

    /// A running dictation; only ever touched on the main thread.
    struct Session {
        app: AppHandle,
        label: String,
        generation: u64,
        recognizer: Obj,
        request: Obj,
        engine: Obj,
        task: Obj,
        stopping: bool,
        last: String,
    }

    thread_local! {
        static SESSION: RefCell<Option<Session>> = const { RefCell::new(None) };
    }

    unsafe fn ns_string(s: &str) -> Obj {
        let Ok(c) = CString::new(s) else {
            return std::ptr::null_mut();
        };
        msg_send![class!(NSString), stringWithUTF8String: c.as_ptr()]
    }

    unsafe fn rust_string(ns: Obj) -> String {
        if ns.is_null() {
            return String::new();
        }
        let utf8: *const c_char = msg_send![ns, UTF8String];
        if utf8.is_null() {
            return String::new();
        }
        CStr::from_ptr(utf8).to_string_lossy().into_owned()
    }

    /// The process macOS holds responsible for this one's permission
    /// requests. The lookup is private, so it is found at run time and falls
    /// back to Wis.
    fn responsible_pid() -> libc::pid_t {
        type Responsible = unsafe extern "C" fn(libc::pid_t) -> libc::pid_t;
        // SAFETY: getpid cannot fail; the symbol, when present, has this signature.
        unsafe {
            let me = libc::getpid();
            let found = libc::dlsym(
                libc::RTLD_DEFAULT,
                c"responsibility_get_pid_responsible_for_pid".as_ptr(),
            );
            if found.is_null() {
                return me;
            }

            let responsible: Responsible = std::mem::transmute(found);
            match responsible(me) {
                pid if pid > 0 => pid,
                _ => me,
            }
        }
    }

    /// Whether macOS will ask rather than end the app: the responsible app
    /// must carry the usage strings. Its name when it does not.
    fn can_ask() -> Result<(), Option<String>> {
        // SAFETY: plain Foundation and AppKit sends; this runs on a worker
        // thread, so its autoreleased objects need a pool of their own.
        autoreleasepool(|_| unsafe {
            let pid = responsible_pid();
            let (bundle, name): (Obj, Option<String>) = if pid == libc::getpid() {
                (msg_send![class!(NSBundle), mainBundle], None)
            } else {
                let app: Obj = msg_send![class!(NSRunningApplication), runningApplicationWithProcessIdentifier: pid];
                if app.is_null() {
                    return Err(None);
                }

                let url: Obj = msg_send![app, bundleURL];
                let bundle: Obj = if url.is_null() {
                    std::ptr::null_mut()
                } else {
                    msg_send![class!(NSBundle), bundleWithURL: url]
                };
                (bundle, Some(rust_string(msg_send![app, localizedName])))
            };
            let carries = !bundle.is_null()
                && [
                    "NSSpeechRecognitionUsageDescription",
                    "NSMicrophoneUsageDescription",
                ]
                .iter()
                .all(|key| {
                    let value: Obj = msg_send![bundle, objectForInfoDictionaryKey: ns_string(key)];
                    !value.is_null()
                });
            if carries {
                Ok(())
            } else {
                Err(name.filter(|n| !n.is_empty()))
            }
        })
    }

    /// Asks for speech recognition; the block is gone before anyone waits.
    fn ask_speech() -> tokio::sync::oneshot::Receiver<isize> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let tx = std::sync::Mutex::new(Some(tx));
        let reply = RcBlock::new(move |status: isize| {
            if let Some(tx) = tx.lock().ok().and_then(|mut t| t.take()) {
                let _ = tx.send(status);
            }
        });
        // SAFETY: the encoding is checked in the tests; the callee copies the block.
        unsafe {
            let _: () = msg_send![class!(SFSpeechRecognizer), requestAuthorization: &*reply];
        }
        rx
    }

    fn ask_microphone() -> tokio::sync::oneshot::Receiver<bool> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let tx = std::sync::Mutex::new(Some(tx));
        let reply = RcBlock::new(move |granted: Bool| {
            if let Some(tx) = tx.lock().ok().and_then(|mut t| t.take()) {
                let _ = tx.send(granted.as_bool());
            }
        });
        // SAFETY: as above.
        unsafe {
            let _: () = msg_send![
                class!(AVCaptureDevice),
                requestAccessForMediaType: AVMediaTypeAudio,
                completionHandler: &*reply
            ];
        }
        rx
    }

    pub async fn authorize() -> Result<(), String> {
        if let Err(host) = can_ask() {
            return Err(super::cannot_ask(host.as_deref()));
        }

        // SAFETY: class methods with the encodings checked in the tests.
        let speech: isize = unsafe { msg_send![class!(SFSpeechRecognizer), authorizationStatus] };
        let speech = if speech == NOT_DETERMINED {
            ask_speech().await.unwrap_or(NOT_DETERMINED)
        } else {
            speech
        };
        if speech != AUTHORIZED {
            return Err(
                "Wis needs Speech Recognition: System Settings › Privacy & Security".into(),
            );
        }

        let mic: isize = unsafe {
            msg_send![class!(AVCaptureDevice), authorizationStatusForMediaType: AVMediaTypeAudio]
        };
        let mic = if mic == NOT_DETERMINED {
            ask_microphone().await.unwrap_or(false)
        } else {
            mic == AUTHORIZED
        };
        if !mic {
            return Err("Wis needs the microphone: System Settings › Privacy & Security".into());
        }

        Ok(())
    }

    /// A reference the tap block keeps for as long as the engine keeps the
    /// block, since a late tap callback may still be running when `end`
    /// releases the session's own.
    struct Held(Obj);

    impl Drop for Held {
        fn drop(&mut self) {
            // SAFETY: the reference retained when the block was made.
            unsafe { release(self.0) }
        }
    }

    unsafe fn release(obj: Obj) {
        if !obj.is_null() {
            let _: () = msg_send![obj, release];
        }
    }

    /// Opens the microphone and starts recognising, on the main thread.
    pub fn begin(app: AppHandle, label: String, generation: u64) -> Result<(), &'static str> {
        if SESSION.with(|s| s.borrow().is_some()) {
            return Err("dictation is already on");
        }
        // SAFETY: every send matches its method (see the tests); objects made
        // here are owned by the session and released in `end`.
        unsafe {
            let recognizer: Obj = msg_send![class!(SFSpeechRecognizer), alloc];
            let recognizer: Obj = msg_send![recognizer, init];
            if recognizer.is_null() {
                return Err("dictation is not available for your language");
            }

            let on_device: bool = msg_send![recognizer, supportsOnDeviceRecognition];
            let available: bool = msg_send![recognizer, isAvailable];
            if !on_device || !available {
                release(recognizer);
                return Err(
                    "on-device dictation is not set up for your language: turn on Dictation in System Settings › Keyboard",
                );
            }

            let request: Obj = msg_send![class!(SFSpeechAudioBufferRecognitionRequest), new];
            let _: () = msg_send![request, setShouldReportPartialResults: Bool::YES];
            let _: () = msg_send![request, setRequiresOnDeviceRecognition: Bool::YES];
            let punctuates: bool =
                msg_send![request, respondsToSelector: sel!(setAddsPunctuation:)];
            if punctuates {
                let _: () = msg_send![request, setAddsPunctuation: Bool::YES];
            }

            let engine: Obj = msg_send![class!(AVAudioEngine), new];
            let input: Obj = msg_send![engine, inputNode];
            let format: Obj = msg_send![input, outputFormatForBus: 0usize];
            let rate: f64 = if format.is_null() {
                0.0
            } else {
                msg_send![format, sampleRate]
            };
            if input.is_null() || rate <= 0.0 {
                release(engine);
                release(request);
                release(recognizer);
                return Err("no microphone is connected");
            }

            let feed = Held(msg_send![request, retain]);
            let tap = RcBlock::new(move |buffer: Obj, _when: Obj| {
                let _: () = msg_send![feed.0, appendAudioPCMBuffer: buffer];
            });
            let _: () = msg_send![
                input,
                installTapOnBus: 0usize,
                bufferSize: TAP_FRAMES,
                format: format,
                block: &*tap
            ];
            let _: () = msg_send![engine, prepare];
            let started: bool = msg_send![engine, startAndReturnError: std::ptr::null_mut::<Obj>()];
            if !started {
                let _: () = msg_send![input, removeTapOnBus: 0usize];
                release(engine);
                release(request);
                release(recognizer);
                return Err("the microphone could not start");
            }

            let heard =
                RcBlock::new(move |result: Obj, error: Obj| hear(generation, result, error));
            let task: Obj =
                msg_send![recognizer, recognitionTaskWithRequest: request, resultHandler: &*heard];
            let task: Obj = if task.is_null() {
                task
            } else {
                msg_send![task, retain]
            };

            SESSION.with(|s| {
                *s.borrow_mut() = Some(Session {
                    app,
                    label,
                    generation,
                    recognizer,
                    request,
                    engine,
                    task,
                    stopping: false,
                    last: String::new(),
                })
            });
        }

        Ok(())
    }

    /// The recogniser's handler, on the main queue.
    fn hear(generation: u64, result: Obj, error: Obj) {
        let found = SESSION.with(|s| {
            let mut s = s.borrow_mut();
            let session = s.as_mut().filter(|x| x.generation == generation)?;
            // SAFETY: result and error are the handler's own arguments.
            let (text, done) = unsafe {
                if result.is_null() {
                    (session.last.clone(), true)
                } else {
                    let best: Obj = msg_send![result, bestTranscription];
                    let text = if best.is_null() {
                        String::new()
                    } else {
                        rust_string(msg_send![best, formattedString])
                    };
                    let done: bool = msg_send![result, isFinal];
                    (text, done)
                }
            };
            session.last.clone_from(&text);
            let code: isize = if error.is_null() {
                0
            } else {
                unsafe { msg_send![error, code] }
            };
            Some((
                session.app.clone(),
                session.label.clone(),
                text,
                done,
                code,
                session.stopping,
            ))
        });
        let Some((app, label, text, done, code, stopping)) = found else {
            return;
        };
        if !error.is_null() {
            if let Some(message) = failure(code, stopping) {
                push(
                    &app,
                    &label,
                    &MicEvent::Failed {
                        message: message.into(),
                    },
                );
            }
            end(&app, generation, false);
            return;
        }
        if !push(&app, &label, &MicEvent::Heard { text, done }) || done {
            end(&app, generation, false);
        }
    }

    /// Stops the microphone; recognition hands over what it has, then ends.
    pub fn stop(_app: &AppHandle, generation: u64) {
        SESSION.with(|s| {
            if let Some(session) = s
                .borrow_mut()
                .as_mut()
                .filter(|x| x.generation == generation)
            {
                if session.stopping {
                    return;
                }

                session.stopping = true;
                // SAFETY: the session's own objects, alive until `end`.
                unsafe {
                    let _: () = msg_send![session.engine, stop];
                    let input: Obj = msg_send![session.engine, inputNode];
                    let _: () = msg_send![input, removeTapOnBus: 0usize];
                    let _: () = msg_send![session.request, endAudio];
                }
            }
        });
    }

    /// Ends the dictation, cancelling recognition if `cancel`, and tells the page.
    pub fn end(app: &AppHandle, generation: u64, cancel: bool) {
        let session = SESSION.with(|s| {
            let mut s = s.borrow_mut();
            if s.as_ref().is_some_and(|x| x.generation == generation) {
                s.take()
            } else {
                None
            }
        });
        let Some(session) = session else {
            release_owner(generation);
            return;
        };
        // SAFETY: the session's own objects, released once here.
        unsafe {
            if !session.stopping {
                let _: () = msg_send![session.engine, stop];
                let input: Obj = msg_send![session.engine, inputNode];
                let _: () = msg_send![input, removeTapOnBus: 0usize];
                let _: () = msg_send![session.request, endAudio];
            }
            if cancel && !session.task.is_null() {
                let _: () = msg_send![session.task, cancel];
            }
            release(session.task);
            release(session.engine);
            release(session.request);
            release(session.recognizer);
        }
        release_owner(generation);
        push(app, &session.label, &MicEvent::Ended);
    }

    #[cfg(test)]
    mod tests {
        use std::ffi::c_char;

        use block2::Block;
        use objc2::runtime::{AnyObject, Bool, Sel};
        use objc2::{class, sel};

        type Obj = *mut AnyObject;

        /// A wrong message type aborts at runtime only, so check each send
        /// against its method.
        #[test]
        fn every_send_matches_its_method() {
            let speech = class!(SFSpeechRecognizer);
            speech
                .metaclass()
                .verify_sel::<(), isize>(sel!(authorizationStatus))
                .unwrap();
            speech
                .metaclass()
                .verify_sel::<(&Block<dyn Fn(isize)>,), ()>(sel!(requestAuthorization:))
                .unwrap();
            speech.verify_sel::<(), Obj>(sel!(init)).unwrap();
            speech
                .verify_sel::<(), Bool>(sel!(supportsOnDeviceRecognition))
                .unwrap();
            speech.verify_sel::<(), Bool>(sel!(isAvailable)).unwrap();
            speech
                .verify_sel::<(Obj, &Block<dyn Fn(Obj, Obj)>), Obj>(sel!(
                    recognitionTaskWithRequest:resultHandler:
                ))
                .unwrap();

            let request = class!(SFSpeechAudioBufferRecognitionRequest);
            request
                .verify_sel::<(Bool,), ()>(sel!(setShouldReportPartialResults:))
                .unwrap();
            request
                .verify_sel::<(Bool,), ()>(sel!(setRequiresOnDeviceRecognition:))
                .unwrap();
            request
                .verify_sel::<(Bool,), ()>(sel!(setAddsPunctuation:))
                .unwrap();
            request
                .verify_sel::<(Obj,), ()>(sel!(appendAudioPCMBuffer:))
                .unwrap();
            request.verify_sel::<(), ()>(sel!(endAudio)).unwrap();
            request
                .verify_sel::<(Sel,), Bool>(sel!(respondsToSelector:))
                .unwrap();

            class!(SFSpeechRecognitionResult)
                .verify_sel::<(), Obj>(sel!(bestTranscription))
                .unwrap();
            class!(SFSpeechRecognitionResult)
                .verify_sel::<(), Bool>(sel!(isFinal))
                .unwrap();
            class!(SFTranscription)
                .verify_sel::<(), Obj>(sel!(formattedString))
                .unwrap();
            class!(SFSpeechRecognitionTask)
                .verify_sel::<(), ()>(sel!(cancel))
                .unwrap();

            let engine = class!(AVAudioEngine);
            engine.verify_sel::<(), Obj>(sel!(inputNode)).unwrap();
            engine.verify_sel::<(), ()>(sel!(prepare)).unwrap();
            engine
                .verify_sel::<(*mut Obj,), Bool>(sel!(startAndReturnError:))
                .unwrap();
            engine.verify_sel::<(), ()>(sel!(stop)).unwrap();
            let node = class!(AVAudioInputNode);
            node.verify_sel::<(usize,), Obj>(sel!(outputFormatForBus:))
                .unwrap();
            node.verify_sel::<(usize, u32, Obj, &Block<dyn Fn(Obj, Obj)>), ()>(sel!(
                installTapOnBus:bufferSize:format:block:
            ))
            .unwrap();
            node.verify_sel::<(usize,), ()>(sel!(removeTapOnBus:))
                .unwrap();
            class!(AVAudioFormat)
                .verify_sel::<(), f64>(sel!(sampleRate))
                .unwrap();

            let capture = class!(AVCaptureDevice).metaclass();
            capture
                .verify_sel::<(Obj,), isize>(sel!(authorizationStatusForMediaType:))
                .unwrap();
            capture
                .verify_sel::<(Obj, &Block<dyn Fn(Bool)>), ()>(sel!(
                    requestAccessForMediaType:completionHandler:
                ))
                .unwrap();

            class!(NSBundle)
                .metaclass()
                .verify_sel::<(), Obj>(sel!(mainBundle))
                .unwrap();
            class!(NSBundle)
                .verify_sel::<(Obj,), Obj>(sel!(objectForInfoDictionaryKey:))
                .unwrap();
            class!(NSError).verify_sel::<(), isize>(sel!(code)).unwrap();
            let running = class!(NSRunningApplication);
            running
                .metaclass()
                .verify_sel::<(i32,), Obj>(sel!(runningApplicationWithProcessIdentifier:))
                .unwrap();
            running.verify_sel::<(), Obj>(sel!(bundleURL)).unwrap();
            running.verify_sel::<(), Obj>(sel!(localizedName)).unwrap();
            class!(NSBundle)
                .metaclass()
                .verify_sel::<(Obj,), Obj>(sel!(bundleWithURL:))
                .unwrap();
            class!(NSString)
                .verify_sel::<(), *const c_char>(sel!(UTF8String))
                .unwrap();
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod native {
    use tauri::AppHandle;

    pub async fn authorize() -> Result<(), String> {
        Err("dictation needs macOS for now".into())
    }

    pub fn begin(_app: AppHandle, _label: String, _generation: u64) -> Result<(), &'static str> {
        Err("dictation needs macOS for now")
    }

    pub fn stop(_app: &AppHandle, _generation: u64) {}

    pub fn end(_app: &AppHandle, _generation: u64, _cancel: bool) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_reach_the_page_tagged() {
        assert_eq!(
            serde_json::to_value(MicEvent::Heard {
                text: "make a timeline".into(),
                done: false
            })
            .unwrap(),
            serde_json::json!({ "type": "heard", "text": "make a timeline", "done": false })
        );
        assert_eq!(
            serde_json::to_value(MicEvent::Ended).unwrap(),
            serde_json::json!({ "type": "ended" })
        );
    }

    #[test]
    fn a_refusal_to_ask_names_the_app_in_the_way() {
        assert!(cannot_ask(Some("Visual Studio Code")).contains("inside Visual Studio Code"));
        assert!(cannot_ask(None).contains("open Wis as an app"));
    }

    #[test]
    fn errors_after_a_stop_or_a_cancel_are_quiet() {
        assert_eq!(failure(1110, false), Some("Didn't hear anything."));
        assert_eq!(failure(1110, true), None);
        assert_eq!(failure(216, false), None);
        assert_eq!(failure(1, false), Some("Dictation stopped."));
    }

    #[test]
    fn the_microphone_is_claimed_before_asking_and_freed_once() {
        *OWNER.lock().unwrap() = None;
        let first = claim("tab-a").unwrap().expect("a new generation");
        assert_eq!(claim("tab-a"), Ok(None), "the same tab already has it");
        assert!(claim("tab-b").is_err(), "another tab waits");
        assert!(still_wanted("tab-a", first));
        assert_eq!(
            ending("tab-a"),
            Some(first),
            "a stop while asking is remembered"
        );
        assert!(
            !still_wanted("tab-a", first),
            "so the microphone never opens"
        );
        assert!(claim("tab-a").is_err(), "nor restarts while it finishes");
        release_owner(first - 1);
        assert!(
            owner().is_some(),
            "an older dictation cannot free a newer one"
        );
        release_owner(first);
        assert_eq!(owner(), None);
        assert_eq!(ending("tab-a"), None);
    }
}

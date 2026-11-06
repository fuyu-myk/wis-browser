//! WebView2's http authentication: each tab defers its
//! `BasicAuthenticationRequested` challenges until the card answers. WebView2
//! names neither the failures nor the page being opened.

use std::cell::RefCell;
use std::rc::Rc;

use tauri::{AppHandle, Manager};
use webview2_com::Microsoft::Web::WebView2::Win32::{
    ICoreWebView2, ICoreWebView2BasicAuthenticationRequestedEventArgs, ICoreWebView2Deferral,
    ICoreWebView2NavigationCompletedEventArgs, ICoreWebView2NavigationStartingEventArgs,
    ICoreWebView2_10,
};
use webview2_com::{
    BasicAuthenticationRequestedEventHandler, CoTaskMemPWSTR, NavigationCompletedEventHandler,
    NavigationStartingEventHandler,
};
use windows::core::{Interface, PCWSTR, PWSTR};
use zeroize::Zeroizing;

use super::{prompted_realm, same_host, Answer, Challenge, Opening, Space, Tried};

type Args = ICoreWebView2BasicAuthenticationRequestedEventArgs;

thread_local! {
    static TRIED: RefCell<Tried> = RefCell::new(Tried::default());
}

pub struct Reply {
    args: Args,
    deferral: ICoreWebView2Deferral,
    space: Space,
    signed_in: bool,
}

/// `text` as a NUL-terminated UTF-16 string.
fn wide(text: &str) -> Zeroizing<Vec<u16>> {
    let mut buffer = Zeroizing::new(Vec::with_capacity(text.len() + 1));
    buffer.extend(text.encode_utf16().chain(Some(0)));
    buffer
}

impl Answer for Reply {
    fn sign_in(mut self, username: &str, password: &str) {
        let (username, password) = (wide(username), wide(password));
        // SAFETY: answers run on the UI thread, where WebView2 raised the
        // challenge; both buffers are NUL-terminated and outlive the calls.
        let set = unsafe {
            self.args.Response().and_then(|response| {
                response.SetUserName(PCWSTR(username.as_ptr()))?;
                response.SetPassword(PCWSTR(password.as_ptr()))
            })
        };
        match set {
            Ok(()) => {
                self.signed_in = true;
                TRIED.with_borrow_mut(|tried| tried.signed_in(self.space.clone()));
            }
            Err(err) => log::warn!("sign-in not passed to WebView2: {err}"),
        }
    }

    fn cancel(self) {
        drop(self);
    }
}

impl Drop for Reply {
    fn drop(&mut self) {
        // SAFETY: replies are only dropped on the UI thread. Completing a
        // deferral with neither credentials nor a cancel would raise
        // WebView2's own dialog, so an unanswered challenge is cancelled.
        unsafe {
            if !self.signed_in {
                if let Err(err) = self.args.SetCancel(true) {
                    log::warn!("sign-in challenge not cancelled: {err}");
                }
            }
            if let Err(err) = self.deferral.Complete() {
                log::warn!("sign-in challenge not completed: {err}");
            }
        }
    }
}

pub(super) fn forget(label: &str) {
    TRIED.with_borrow_mut(|tried| tried.forget(label));
}

/// Takes over a webview's sign-in challenges, on the UI thread.
/// `first_page` is the page it was built to load, whose navigation may start
/// before the handlers are in.
pub fn attach(webview: &tauri::Webview, first_page: Option<&str>) {
    let app = webview.app_handle().clone();
    let label = webview.label().to_owned();
    let first_page = first_page.map(str::to_owned);
    crate::platform::win_webview2::with_core_webview2(webview, move |core| {
        let mut seeded = Opening::default();
        if let Some(url) = first_page.filter(|_| !shows_a_page(core)) {
            seeded.opened(url);
        }
        let opening = Rc::new(RefCell::new(seeded));
        if let Err(err) = watch_navigation(core, &opening) {
            log::warn!("sign-in prompts cannot follow navigation on {label}: {err}");
        }
        if let Err(err) = watch_challenges(core, app, label.clone(), opening) {
            log::warn!("sign-in prompts off for {label}: {err}");
        }
    });
}

/// Takes a string WebView2 allocated for an out-parameter.
fn taken(text: PWSTR) -> String {
    CoTaskMemPWSTR::from(text).to_string()
}

/// Whether a page has committed already.
fn shows_a_page(core: &ICoreWebView2) -> bool {
    let mut source = PWSTR::null();
    // SAFETY: UI thread; the out-param is a stack local, taken over by `taken`.
    let outcome = unsafe { core.Source(&mut source) };
    let source = taken(source);
    outcome.is_ok() && !source.is_empty() && source != "about:blank"
}

fn watch_navigation(
    core: &ICoreWebView2,
    opening: &Rc<RefCell<Opening>>,
) -> windows::core::Result<()> {
    let started = Rc::clone(opening);
    let starting = NavigationStartingEventHandler::create(Box::new(
        move |_sender: Option<ICoreWebView2>,
              args: Option<ICoreWebView2NavigationStartingEventArgs>|
              -> windows::core::Result<()> {
            let Some(args) = args else {
                return Ok(());
            };
            let (mut navigation, mut uri) = (0_u64, PWSTR::null());
            // SAFETY: UI thread; the out-params are stack locals, and the
            // URI's memory is taken over by `taken`.
            unsafe {
                args.NavigationId(&mut navigation)?;
                args.Uri(&mut uri)?;
            }
            started.borrow_mut().started(navigation, taken(uri));
            Ok(())
        },
    ));
    let mut token = 0_i64;
    // SAFETY: on the UI thread via `with_core_webview2`.
    unsafe { core.add_NavigationStarting(&starting, &mut token)? };

    let ended = Rc::clone(opening);
    let completed = NavigationCompletedEventHandler::create(Box::new(
        move |_sender: Option<ICoreWebView2>,
              args: Option<ICoreWebView2NavigationCompletedEventArgs>|
              -> windows::core::Result<()> {
            let Some(args) = args else {
                return Ok(());
            };
            let mut navigation = 0_u64;
            // SAFETY: UI thread; the out-param is a stack local.
            unsafe { args.NavigationId(&mut navigation)? };
            ended.borrow_mut().completed(navigation);
            Ok(())
        },
    ));
    // SAFETY: as above.
    unsafe { core.add_NavigationCompleted(&completed, &mut token) }
}

fn watch_challenges(
    core: &ICoreWebView2,
    app: AppHandle,
    label: String,
    opening: Rc<RefCell<Opening>>,
) -> windows::core::Result<()> {
    let core = core.cast::<ICoreWebView2_10>()?;
    let handler = BasicAuthenticationRequestedEventHandler::create(Box::new(
        move |sender: Option<ICoreWebView2>, args: Option<Args>| -> windows::core::Result<()> {
            let (Some(sender), Some(args)) = (sender, args) else {
                return Ok(());
            };
            challenged(&app, &label, &opening, &sender, args)
        },
    ));
    let mut token = 0_i64;
    // SAFETY: on the UI thread via `with_core_webview2`.
    unsafe { core.add_BasicAuthenticationRequested(&handler, &mut token) }
}

/// Shows the server's 401 page instead of any dialog.
fn refuse(args: &Args) -> windows::core::Result<()> {
    // SAFETY: on the UI thread, inside the event.
    unsafe { args.SetCancel(true) }
}

/// The challenge's URI and text, and the URL of the page committed.
fn read(sender: &ICoreWebView2, args: &Args) -> windows::core::Result<(String, String, String)> {
    let (mut uri, mut text, mut source) = (PWSTR::null(), PWSTR::null(), PWSTR::null());
    // SAFETY: UI thread; the out-params are stack locals, each taken over
    // below whichever call fails.
    let outcome = unsafe {
        args.Uri(&mut uri)
            .and_then(|()| args.Challenge(&mut text))
            .and_then(|()| sender.Source(&mut source))
    };
    let strings = (taken(uri), taken(text), taken(source));
    outcome.map(|()| strings)
}

/// A challenge another page's resource raises, or one Wis cannot read, is
/// refused.
fn challenged(
    app: &AppHandle,
    label: &str,
    opening: &RefCell<Opening>,
    sender: &ICoreWebView2,
    args: Args,
) -> windows::core::Result<()> {
    let Ok((uri, text, source)) = read(sender, &args) else {
        return refuse(&args);
    };
    let Ok(target) = url::Url::parse(&uri) else {
        return refuse(&args);
    };
    let Some(host) = target.host_str().map(str::to_owned) else {
        return refuse(&args);
    };
    let on_page = same_host(Some(&source), &host) || same_host(opening.borrow().url(), &host);
    let Some(realm) = prompted_realm(&text) else {
        return if on_page && super::prompts_for(label) {
            Ok(())
        } else {
            refuse(&args)
        };
    };
    // SAFETY: UI thread; the deferral is completed when the reply drops.
    let Ok(deferral) = (unsafe { args.GetDeferral() }) else {
        return refuse(&args);
    };

    let space = Space {
        label: label.to_owned(),
        host,
        port: target
            .port_or_known_default()
            .and_then(|port| isize::try_from(port).ok())
            .unwrap_or_default(),
        protocol: target.scheme().to_owned(),
        realm,
    };
    let failed = TRIED.with_borrow_mut(|tried| tried.failed(&space));
    let reply = Reply {
        args,
        deferral,
        space: space.clone(),
        signed_in: false,
    };
    let challenge = Challenge {
        space,
        on_page,
        failed,
    };
    super::offer(app, challenge, reply);

    Ok(())
}

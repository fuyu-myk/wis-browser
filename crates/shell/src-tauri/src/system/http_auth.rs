//! Sign-in prompts for sites that ask for a username and password (HTTP Basic
//! and Digest). WebKit hands each challenge to wry's hook on the main thread;
//! the reply waits here, on the main thread, until the prompt overlay answers
//! through [`http_auth_respond`]. Challenges for the same webview and realm
//! share one prompt. A webview that closes cancels its waiting challenges.
//! Credentials last until Wis quits.

#[cfg(any(target_os = "macos", test))]
use std::collections::HashMap;

use serde::Deserialize;
#[cfg(target_os = "macos")]
use serde::Serialize;
use tauri::AppHandle;
use wis_core::{WisError, WisResult};
use zeroize::Zeroizing;

/// Event that raises the sign-in prompt in `window`.
#[cfg(target_os = "macos")]
const REQUEST_EVENT: &str = "wis-http-auth-request";
/// Event that withdraws a prompt whose page went away.
#[cfg(target_os = "macos")]
const DISMISS_EVENT: &str = "wis-http-auth-dismiss";

#[cfg(any(target_os = "macos", test))]
const MAX_WAITING: usize = 8;
/// Most challenges one prompt answers.
#[cfg(any(target_os = "macos", test))]
const MAX_JOINED: usize = 32;
/// The most of a server's realm text the card shows.
#[cfg(any(target_os = "macos", test))]
const MAX_REALM: usize = 80;
const MAX_USERNAME: usize = 1024;
const MAX_PASSWORD: usize = 4096;

/// Where a credential applies: one webview and one server realm.
#[cfg(any(target_os = "macos", test))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Space {
    pub label: String,
    pub host: String,
    pub port: isize,
    pub protocol: String,
    pub realm: Option<String>,
}

/// What became of a challenge handed to [`Prompts::add`].
#[cfg(any(target_os = "macos", test))]
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Added<R> {
    /// It opened prompt `id`.
    New(u64),
    /// It joined prompt `id`, already waiting.
    Joined(u64),
    /// It was refused, and is handed back to be cancelled.
    Refused(R),
}

/// The prompts waiting for an answer, each with every reply it will answer.
#[cfg(any(target_os = "macos", test))]
pub(crate) struct Prompts<R> {
    next: u64,
    waiting: HashMap<u64, (Space, Vec<R>)>,
}

#[cfg(any(target_os = "macos", test))]
impl<R> Default for Prompts<R> {
    fn default() -> Self {
        Self {
            next: 0,
            waiting: HashMap::new(),
        }
    }
}

#[cfg(any(target_os = "macos", test))]
impl<R> Prompts<R> {
    pub fn add(&mut self, space: Space, reply: R) -> Added<R> {
        if let Some((id, (_, replies))) = self.waiting.iter_mut().find(|(_, (s, _))| *s == space) {
            if replies.len() >= MAX_JOINED {
                return Added::Refused(reply);
            }
            replies.push(reply);
            return Added::Joined(*id);
        }

        let on_tab = self
            .waiting
            .values()
            .filter(|(s, _)| s.label == space.label)
            .count();
        if on_tab >= MAX_WAITING {
            return Added::Refused(reply);
        }

        let id = self.next;
        self.next += 1;
        self.waiting.insert(id, (space, vec![reply]));
        Added::New(id)
    }

    /// Every reply a prompt answers.
    pub fn take(&mut self, id: u64) -> Vec<R> {
        self.waiting
            .remove(&id)
            .map(|(_, replies)| replies)
            .unwrap_or_default()
    }

    /// The prompts of a webview that went away, with their replies.
    pub fn take_label(&mut self, label: &str) -> Vec<(u64, Vec<R>)> {
        let ids: Vec<u64> = self
            .waiting
            .iter()
            .filter(|(_, (space, _))| space.label == label)
            .map(|(id, _)| *id)
            .collect();
        ids.into_iter().map(|id| (id, self.take(id))).collect()
    }
}

/// A server's realm as the card shows it: trimmed, and cut short.
#[cfg(any(target_os = "macos", test))]
fn shown_realm(realm: Option<String>) -> Option<String> {
    let realm = realm?;
    let realm = realm.trim();
    if realm.is_empty() {
        return None;
    }
    if realm.chars().count() <= MAX_REALM {
        return Some(realm.to_owned());
    }
    Some(realm.chars().take(MAX_REALM).chain(['…']).collect())
}

/// Whether a challenge from `host` comes from the host of the page the tab is
/// on, or is loading. A third party's image or frame never prompts; as in
/// Chromium and Firefox, that holds for sibling hosts of one site too.
#[cfg(any(target_os = "macos", test))]
fn same_host(page_url: Option<&str>, host: &str) -> bool {
    use url::Host;

    /// Lowercased and punycoded by the parse; a trailing dot names the same host.
    fn plain(host: Host<String>) -> Host<String> {
        match host {
            Host::Domain(name) => Host::Domain(name.trim_end_matches('.').to_owned()),
            other => other,
        }
    }

    let Some(page) = page_url.and_then(|u| url::Url::parse(u).ok()) else {
        return false;
    };
    let Some(page_host) =
        wis_core::sites::domain::site_url(&page).and_then(|u| u.host().map(|h| h.to_owned()))
    else {
        return false;
    };
    let bracketed = if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]")
    } else {
        host.to_owned()
    };

    Host::parse(&bracketed).is_ok_and(|challenge| plain(challenge) == plain(page_host))
}

/// What the prompt sends back when the user signs in.
#[derive(Deserialize)]
pub struct HttpCredentials {
    pub username: String,
    pub password: String,
}

/// The username and password, checked, or why they cannot be sent.
fn checked(credentials: HttpCredentials) -> WisResult<(Zeroizing<String>, Zeroizing<String>)> {
    let username = Zeroizing::new(credentials.username);
    let password = Zeroizing::new(credentials.password);
    let refuse = |message: &str| {
        Err(WisError::InvalidInput {
            message: message.into(),
        })
    };
    if username.contains(':') {
        return refuse("a username cannot contain a colon");
    }
    if username.chars().count() > MAX_USERNAME || password.chars().count() > MAX_PASSWORD {
        return refuse("that username or password is too long");
    }
    Ok((username, password))
}

/// The prompt shown in `window` for one challenge.
#[cfg(target_os = "macos")]
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct HttpAuthRequest {
    request_id: u64,
    window: String,
    host: String,
    realm: Option<String>,
    /// The page is plain `http:`, so the password travels unencrypted.
    insecure: bool,
    /// The last username and password were refused.
    failed: bool,
}

/// Answers a prompt: signs in with `credentials`, or cancels without them.
#[tauri::command]
pub async fn http_auth_respond(
    app: AppHandle,
    request_id: u64,
    credentials: Option<HttpCredentials>,
) -> WisResult<()> {
    let credentials = credentials.map(checked).transpose()?;

    #[cfg(target_os = "macos")]
    app.run_on_main_thread(move || imp::answer(request_id, credentials))
        .map_err(|err| WisError::InvalidInput {
            message: format!("run_on_main_thread: {err}"),
        })?;

    #[cfg(not(target_os = "macos"))]
    let _ = (app, request_id, credentials);

    Ok(())
}

#[cfg(target_os = "macos")]
pub use imp::{cancel_all, forget, install};

#[cfg(target_os = "macos")]
mod imp {
    use std::cell::RefCell;

    use tauri::{AppHandle, Emitter, Manager};
    use zeroize::Zeroizing;

    use super::{
        same_host, shown_realm, Added, HttpAuthRequest, Prompts, Space, DISMISS_EVENT,
        REQUEST_EVENT,
    };

    thread_local! {
        /// Main thread only, as WebKit's completion handlers are.
        static PROMPTS: RefCell<Prompts<wry::AuthReply>> = RefCell::new(Prompts::default());
    }

    /// Tabs prompt; Peek, widget panels and Wis's hidden pages show the
    /// server's refusal instead.
    fn prompts_for(label: &str) -> bool {
        label.starts_with("tab-")
    }

    /// A browser window, whose chrome shows the card; not a popped-out video.
    fn shows_cards(window: &str) -> bool {
        window == "main" || window.starts_with("win-")
    }

    pub fn install(app: AppHandle) {
        wry::set_auth_challenge_hook(Box::new(move |challenge, reply| {
            // SAFETY: wry calls the hook on the main thread with a live WKWebView.
            let label = unsafe { crate::platform::native_webview::label(challenge.webview) };
            let Some(label) = label.filter(|label| prompts_for(label)) else {
                return reply.cancel();
            };
            let Some(window) = app
                .webviews()
                .get(&label)
                .map(|webview| webview.window().label().to_string())
                .filter(|window| shows_cards(window))
            else {
                return reply.cancel();
            };
            // SAFETY: as above.
            let page = unsafe { crate::platform::native_webview::url(challenge.webview) };
            if !same_host(page.as_deref(), &challenge.host) {
                return reply.cancel();
            }

            let space = Space {
                label,
                host: challenge.host.clone(),
                port: challenge.port,
                protocol: challenge.protocol.clone(),
                realm: challenge.realm.clone(),
            };
            let added = PROMPTS.with_borrow_mut(|prompts| prompts.add(space, reply));
            let request_id = match added {
                Added::New(id) | Added::Joined(id) => id,
                Added::Refused(refused) => return refused.cancel(),
            };
            let request = HttpAuthRequest {
                request_id,
                window,
                host: challenge.host,
                realm: shown_realm(challenge.realm),
                insecure: challenge.protocol == "http",
                failed: challenge.previous_failures > 0,
            };
            if let Err(err) = app.emit(REQUEST_EVENT, request) {
                log::warn!("sign-in prompt for {request_id} not shown: {err}");
                PROMPTS.with_borrow_mut(|prompts| prompts.take(request_id));
            }
        }));
    }

    pub(super) fn answer(
        request_id: u64,
        credentials: Option<(Zeroizing<String>, Zeroizing<String>)>,
    ) {
        let replies = PROMPTS.with_borrow_mut(|prompts| prompts.take(request_id));
        for reply in replies {
            match &credentials {
                Some((username, password)) => reply.sign_in(username, password),
                None => reply.cancel(),
            }
        }
    }

    /// At quit, on the main thread: cancels every waiting challenge while
    /// WebKit can still take the answer.
    pub fn cancel_all() {
        let waiting = PROMPTS.with_borrow_mut(std::mem::take);
        drop(waiting);
    }

    /// For a webview being torn down, on the main thread: cancels what it
    /// waits on and withdraws its prompts.
    pub fn forget(app: &AppHandle, label: &str) {
        let gone = PROMPTS.with_borrow_mut(|prompts| prompts.take_label(label));
        for (request_id, replies) in gone {
            drop(replies);
            if let Err(err) = app.emit(DISMISS_EVENT, request_id) {
                log::warn!("sign-in prompt {request_id} not withdrawn: {err}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn space(label: &str, realm: &str) -> Space {
        Space {
            label: label.into(),
            host: "pymol.org".into(),
            port: 443,
            protocol: "https".into(),
            realm: Some(realm.into()),
        }
    }

    #[test]
    fn challenges_for_one_page_and_realm_share_a_prompt() {
        let mut prompts = Prompts::default();
        let Added::New(first) = prompts.add(space("tab-a", "ep"), 1) else {
            panic!("a prompt")
        };
        assert_eq!(prompts.add(space("tab-a", "ep"), 2), Added::Joined(first));
        assert!(matches!(prompts.add(space("tab-a", "admin"), 3), Added::New(id) if id != first));
        assert!(matches!(prompts.add(space("tab-b", "ep"), 4), Added::New(id) if id != first));
        assert_eq!(prompts.take(first), vec![1, 2]);
        assert!(prompts.take(first).is_empty(), "answered once");
    }

    #[test]
    fn a_closing_page_takes_its_prompts_with_it() {
        let mut prompts = Prompts::default();
        let Added::New(a) = prompts.add(space("tab-a", "ep"), 1) else {
            panic!("a prompt")
        };
        prompts.add(space("tab-b", "ep"), 2);
        assert_eq!(prompts.take_label("tab-a"), vec![(a, vec![1])]);
        assert!(prompts.take_label("tab-a").is_empty());
        assert_eq!(prompts.take_label("tab-b").len(), 1);
    }

    #[test]
    fn one_page_cannot_stack_endless_prompts_or_crowd_out_others() {
        let mut prompts = Prompts::default();
        for realm in 0..MAX_WAITING {
            assert!(matches!(
                prompts.add(space("tab-a", &realm.to_string()), realm),
                Added::New(_)
            ));
        }
        assert_eq!(
            prompts.add(space("tab-a", "one more"), 99),
            Added::Refused(99)
        );
        assert!(matches!(
            prompts.add(space("tab-b", "ep"), 100),
            Added::New(_)
        ));
        assert!(matches!(
            prompts.add(space("tab-a", "0"), 101),
            Added::Joined(_)
        ));
    }

    #[test]
    fn a_prompt_holds_a_bounded_number_of_requests() {
        let mut prompts = Prompts::default();
        for n in 0..MAX_JOINED {
            assert!(!matches!(
                prompts.add(space("tab-a", "ep"), n),
                Added::Refused(_)
            ));
        }
        assert_eq!(prompts.add(space("tab-a", "ep"), 999), Added::Refused(999));
    }

    #[test]
    fn a_long_realm_is_cut_for_the_card() {
        assert_eq!(
            shown_realm(Some("PyMOL Educational Products".into())).as_deref(),
            Some("PyMOL Educational Products")
        );
        let long = shown_realm(Some("x".repeat(500))).unwrap();
        assert_eq!(long.chars().count(), MAX_REALM + 1);
        assert!(long.ends_with('…'));
        assert_eq!(shown_realm(Some("  ".into())), None);
    }

    #[test]
    fn only_the_page_s_own_host_may_prompt() {
        assert!(same_host(Some("https://pymol.org/ep/"), "pymol.org"));
        assert!(same_host(Some("https://PyMOL.org./ep/"), "pymol.org"));
        assert!(same_host(Some("blob:https://pymol.org/0f3c"), "pymol.org"));
        assert!(same_host(Some("http://[::1]:8080/"), "::1"));
        assert!(same_host(Some("http://127.0.0.1:8080/"), "127.0.0.1"));
        assert!(!same_host(
            Some("https://www.pymol.org/"),
            "files.pymol.org"
        ));
        assert!(!same_host(
            Some("https://victim.github.io/"),
            "evil.github.io"
        ));
        assert!(!same_host(Some("https://news.example/"), "pymol.org"));
        assert!(!same_host(Some("about:blank"), "pymol.org"));
        assert!(!same_host(None, "pymol.org"));
    }

    #[test]
    fn a_username_with_a_colon_cannot_be_sent() {
        let ok = |u: &str, p: &str| {
            checked(HttpCredentials {
                username: u.into(),
                password: p.into(),
            })
            .is_ok()
        };
        assert!(ok("student", "s3cret:with:colons"));
        assert!(
            !ok("a:b", "x"),
            "Basic cannot carry a colon in the username"
        );
        assert!(!ok(&"u".repeat(MAX_USERNAME + 1), "x"));
        assert!(!ok("u", &"p".repeat(MAX_PASSWORD + 1)));
    }
}

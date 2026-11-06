//! Sign-in prompts for sites that ask for a username and password (HTTP Basic
//! and Digest). Each engine hands its challenge over on the main thread
//! (`mac`, `win`, `linux`); the reply waits here, on the main thread, until the
//! prompt overlay answers through [`http_auth_respond`]. Challenges for the
//! same webview and realm share one prompt. A webview that closes cancels its
//! waiting challenges. Credentials last until Wis quits.

use std::cell::RefCell;
use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};
use wis_core::{WisError, WisResult};
use zeroize::Zeroizing;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod mac;
#[cfg(windows)]
mod win;

#[cfg(target_os = "linux")]
use linux as platform;
#[cfg(target_os = "macos")]
use mac as platform;
#[cfg(windows)]
use win as platform;

#[cfg(any(windows, target_os = "linux"))]
pub use platform::attach;
#[cfg(target_os = "macos")]
pub use platform::install;

/// Event that raises the sign-in prompt in `window`.
const REQUEST_EVENT: &str = "wis-http-auth-request";
/// Event that withdraws a prompt whose page went away.
const DISMISS_EVENT: &str = "wis-http-auth-dismiss";

const MAX_WAITING: usize = 8;
/// Most challenges one prompt answers.
const MAX_JOINED: usize = 32;
/// The most of a server's realm text the card shows.
const MAX_REALM: usize = 80;
#[cfg(any(windows, test))]
const MAX_TRIED: usize = 32;
const MAX_USERNAME: usize = 1024;
const MAX_PASSWORD: usize = 4096;

/// Where a credential applies: one webview and one server realm.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Space {
    pub label: String,
    pub host: String,
    pub port: isize,
    pub protocol: String,
    pub realm: Option<String>,
}

/// One challenge, as an engine reports it.
pub(crate) struct Challenge {
    pub space: Space,
    /// It comes from the host of the page the tab is on, or is opening.
    pub on_page: bool,
    /// The last username and password were refused.
    pub failed: bool,
}

/// An engine's answer to one challenge. Dropping it unanswered shows the
/// server's refusal.
pub(crate) trait Answer {
    fn sign_in(self, username: &str, password: &str);
    fn cancel(self);
}

/// What became of a challenge handed to [`Prompts::add`].
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
pub(crate) struct Prompts<R> {
    next: u64,
    waiting: HashMap<u64, (Space, Vec<R>)>,
}

impl<R> Default for Prompts<R> {
    fn default() -> Self {
        Self {
            next: 0,
            waiting: HashMap::new(),
        }
    }
}

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

    /// The prompt for `space`, if one waits, with its replies.
    #[cfg(any(target_os = "linux", test))]
    pub fn take_space(&mut self, space: &Space) -> Option<(u64, Vec<R>)> {
        let id = self
            .waiting
            .iter()
            .find(|(_, (s, _))| s == space)
            .map(|(id, _)| *id)?;
        Some((id, self.take(id)))
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

/// The spaces signed in to on each tab, so another challenge for one reads
/// as a refused password. WebView2 does not count failures itself.
#[cfg(any(windows, test))]
#[derive(Default)]
pub(crate) struct Tried(Vec<Space>);

#[cfg(any(windows, test))]
impl Tried {
    pub fn signed_in(&mut self, space: Space) {
        self.0.retain(|s| *s != space);
        if self.0.len() >= MAX_TRIED {
            self.0.remove(0);
        }
        self.0.push(space);
    }

    /// Whether `space` was signed in to; asking forgets it.
    pub fn failed(&mut self, space: &Space) -> bool {
        let before = self.0.len();
        self.0.retain(|s| s != space);
        self.0.len() != before
    }

    pub fn forget(&mut self, label: &str) {
        self.0.retain(|s| s.label != label);
    }
}

/// The main-frame navigation a WebView2 tab has under way. `Source` names
/// the page only once it commits.
#[cfg(any(windows, test))]
#[derive(Default)]
pub(crate) struct Opening(Option<(Option<u64>, String)>);

#[cfg(any(windows, test))]
impl Opening {
    /// The page a webview was built to load, whose navigation id is unknown.
    pub fn opened(&mut self, url: String) {
        self.0 = Some((None, url));
    }

    pub fn started(&mut self, navigation: u64, url: String) {
        self.0 = Some((Some(navigation), url));
    }

    pub fn completed(&mut self, navigation: u64) {
        if self
            .0
            .as_ref()
            .is_some_and(|(open, _)| open.is_none_or(|open| open == navigation))
        {
            self.0 = None;
        }
    }

    pub fn url(&self) -> Option<&str> {
        self.0.as_ref().map(|(_, url)| url.as_str())
    }
}

/// The realm a WebView2 challenge string names, if it asks for Basic or
/// Digest: `None` leaves another scheme (NTLM, Negotiate) to the engine.
#[cfg(any(windows, test))]
fn prompted_realm(challenge: &str) -> Option<Option<String>> {
    let text = challenge.trim();
    let (first, rest) = text.split_once(char::is_whitespace).unwrap_or((text, ""));
    let params = if first.contains('=') {
        text
    } else if first.eq_ignore_ascii_case("basic") || first.eq_ignore_ascii_case("digest") {
        rest
    } else {
        return None;
    };
    Some(auth_param(params, "realm"))
}

/// One `name=value` parameter of a challenge, the value unquoted.
#[cfg(any(windows, test))]
fn auth_param(mut params: &str, wanted: &str) -> Option<String> {
    loop {
        params = params.trim_start_matches(|c: char| c == ',' || c.is_whitespace());
        let (name, rest) = params.split_once('=')?;
        let rest = rest.trim_start();
        let value = if let Some(quoted) = rest.strip_prefix('"') {
            let mut value = String::new();
            let mut chars = quoted.char_indices();
            params = "";
            while let Some((at, c)) = chars.next() {
                match c {
                    '\\' => value.extend(chars.next().map(|(_, escaped)| escaped)),
                    '"' => {
                        params = &quoted[at + 1..];
                        break;
                    }
                    c => value.push(c),
                }
            }
            value
        } else {
            let end = rest.find(',').unwrap_or(rest.len());
            params = &rest[end..];
            rest[..end].trim().to_owned()
        };
        if name.trim().eq_ignore_ascii_case(wanted) {
            return Some(value);
        }
    }
}

/// A server's realm as the card shows it: trimmed, and cut short.
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

/// Tabs prompt; Peek, widget panels and Wis's hidden pages show the
/// server's refusal instead.
fn prompts_for(label: &str) -> bool {
    label.starts_with("tab-")
}

/// A browser window, whose chrome shows the card; not a popped-out video.
fn shows_cards(window: &str) -> bool {
    window == "main" || window.starts_with("win-")
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

thread_local! {
    /// Main thread only, as the engines' replies are.
    static PROMPTS: RefCell<Prompts<platform::Reply>> = RefCell::new(Prompts::default());
}

/// On the main thread: raises the card for `challenge`, or answers it
/// without credentials when this page may not prompt.
fn offer(app: &AppHandle, challenge: Challenge, reply: platform::Reply) {
    let Challenge {
        space,
        on_page,
        failed,
    } = challenge;
    let window = app
        .webviews()
        .get(&space.label)
        .map(|webview| webview.window().label().to_string())
        .filter(|window| on_page && prompts_for(&space.label) && shows_cards(window));
    let Some(window) = window else {
        return Answer::cancel(reply);
    };

    let host = space.host.clone();
    let realm = shown_realm(space.realm.clone());
    let insecure = space.protocol == "http";
    let added = PROMPTS.with_borrow_mut(|prompts| prompts.add(space, reply));
    let request_id = match added {
        Added::New(id) | Added::Joined(id) => id,
        Added::Refused(refused) => return Answer::cancel(refused),
    };
    let request = HttpAuthRequest {
        request_id,
        window,
        host,
        realm,
        insecure,
        failed,
    };
    if let Err(err) = app.emit(REQUEST_EVENT, request) {
        log::warn!("sign-in prompt for {request_id} not shown: {err}");
        PROMPTS.with_borrow_mut(|prompts| prompts.take(request_id));
        dismiss(app, request_id);
    }
}

fn dismiss(app: &AppHandle, request_id: u64) {
    if let Err(err) = app.emit(DISMISS_EVENT, request_id) {
        log::warn!("sign-in prompt {request_id} not withdrawn: {err}");
    }
}

fn answer(request_id: u64, credentials: Option<(Zeroizing<String>, Zeroizing<String>)>) {
    let replies = PROMPTS.with_borrow_mut(|prompts| prompts.take(request_id));
    for reply in replies {
        match &credentials {
            Some((username, password)) => Answer::sign_in(reply, username, password),
            None => Answer::cancel(reply),
        }
    }
}

/// At quit, on the main thread: cancels every waiting challenge while the
/// engine can still take the answer.
pub fn cancel_all() {
    let waiting = PROMPTS.with_borrow_mut(std::mem::take);
    drop(waiting);
}

/// For a webview being torn down or navigating, on the main thread: cancels
/// what it waits on and withdraws its prompts.
pub fn forget(app: &AppHandle, label: &str) {
    #[cfg(windows)]
    win::forget(label);
    let gone = PROMPTS.with_borrow_mut(|prompts| prompts.take_label(label));
    for (request_id, replies) in gone {
        drop(replies);
        dismiss(app, request_id);
    }
}

/// For a challenge the engine gave up on, on the main thread: withdraws the
/// prompt it waits in.
#[cfg(target_os = "linux")]
fn withdraw(app: &AppHandle, space: &Space) {
    let Some((request_id, replies)) = PROMPTS.with_borrow_mut(|prompts| prompts.take_space(space))
    else {
        return;
    };
    drop(replies);
    dismiss(app, request_id);
}

/// Answers a prompt: signs in with `credentials`, or cancels without them.
#[tauri::command]
pub async fn http_auth_respond(
    app: AppHandle,
    request_id: u64,
    credentials: Option<HttpCredentials>,
) -> WisResult<()> {
    let credentials = credentials.map(checked).transpose()?;

    app.run_on_main_thread(move || answer(request_id, credentials))
        .map_err(|err| WisError::InvalidInput {
            message: format!("run_on_main_thread: {err}"),
        })
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
    fn a_challenge_the_engine_gave_up_on_takes_its_prompt() {
        let mut prompts = Prompts::default();
        let Added::New(ep) = prompts.add(space("tab-a", "ep"), 1) else {
            panic!("a prompt")
        };
        prompts.add(space("tab-a", "ep"), 2);
        prompts.add(space("tab-a", "admin"), 3);
        assert_eq!(
            prompts.take_space(&space("tab-a", "ep")),
            Some((ep, vec![1, 2]))
        );
        assert_eq!(prompts.take_space(&space("tab-a", "ep")), None);
        assert_eq!(
            prompts.take_label("tab-a").len(),
            1,
            "the other realm waits"
        );
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
    fn a_second_challenge_after_signing_in_reads_as_a_refusal() {
        let mut tried = Tried::default();
        assert!(!tried.failed(&space("tab-a", "ep")));
        tried.signed_in(space("tab-a", "ep"));
        assert!(!tried.failed(&space("tab-a", "admin")));
        assert!(!tried.failed(&space("tab-b", "ep")));
        assert!(tried.failed(&space("tab-a", "ep")));
        assert!(
            !tried.failed(&space("tab-a", "ep")),
            "a cancelled retry starts afresh"
        );
    }

    #[test]
    fn a_page_that_moves_on_forgets_what_it_signed_in_to() {
        let mut tried = Tried::default();
        tried.signed_in(space("tab-a", "ep"));
        tried.signed_in(space("tab-b", "ep"));
        tried.forget("tab-a");
        assert!(!tried.failed(&space("tab-a", "ep")));
        assert!(tried.failed(&space("tab-b", "ep")));
    }

    #[test]
    fn signed_in_spaces_are_bounded() {
        let mut tried = Tried::default();
        for realm in 0..=MAX_TRIED {
            tried.signed_in(space("tab-a", &realm.to_string()));
        }
        tried.signed_in(space("tab-a", "5"));
        assert!(!tried.failed(&space("tab-a", "0")), "the oldest went");
        assert!(tried.failed(&space("tab-a", "1")));
        assert!(tried.failed(&space("tab-a", "5")));
        assert!(tried.failed(&space("tab-a", &MAX_TRIED.to_string())));
    }

    #[test]
    fn the_opening_page_lasts_until_its_own_navigation_ends() {
        let mut opening = Opening::default();
        assert_eq!(opening.url(), None);
        opening.started(1, "https://pymol.org/ep".into());
        opening.started(2, "https://news.example/".into());
        opening.completed(1);
        assert_eq!(opening.url(), Some("https://news.example/"));
        opening.completed(2);
        assert_eq!(opening.url(), None);
    }

    #[test]
    fn a_new_tab_s_first_page_counts_until_any_navigation_ends() {
        let mut opening = Opening::default();
        opening.opened("https://pymol.org/ep".into());
        assert_eq!(opening.url(), Some("https://pymol.org/ep"));
        opening.completed(7);
        assert_eq!(opening.url(), None, "its id was never seen");

        opening.opened("https://pymol.org/ep".into());
        opening.started(8, "https://news.example/".into());
        opening.completed(7);
        assert_eq!(opening.url(), Some("https://news.example/"));
    }

    #[test]
    fn webview2_challenges_name_their_scheme_and_realm() {
        let realm = |text: &str| prompted_realm(text);
        let named = |name: &str| Some(Some(name.to_owned()));
        assert_eq!(
            realm(r#"Basic realm="PyMOL Educational Products""#),
            named("PyMOL Educational Products")
        );
        assert_eq!(realm("basic realm=intranet"), named("intranet"));
        assert_eq!(
            realm(r#"Digest qop="auth", realm="a \"quoted\" realm", nonce="x""#),
            named(r#"a "quoted" realm"#)
        );
        assert_eq!(
            realm(r#"Digest nonce="realm=fake", realm="real""#),
            named("real")
        );
        assert_eq!(realm(r#"realm="params only""#), named("params only"));
        assert_eq!(realm("Basic"), Some(None));
        assert_eq!(realm(r#"Basic charset="UTF-8""#), Some(None));
        assert_eq!(realm(r#"Basic realm="unclosed"#), named("unclosed"));
        assert_eq!(realm("NTLM"), None);
        assert_eq!(realm("Negotiate abc=="), None);
        assert_eq!(realm(r#"Bearer realm="api""#), None);
        assert_eq!(realm(""), None);
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
        assert!(same_host(Some("http://[::1]:8080/"), "[::1]"));
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

//! Saved addresses and cards at a page's form fields: which fields are for an
//! address or a card, reporting focus on one so the chrome can offer what is
//! saved, and filling the chosen one into the fields around it.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use wis_core::{TabId, WisError, WisResult};

use crate::state::AppState;
use crate::vault::signup::{field_rect, source, FieldRect};

pub(crate) mod card_fill;
mod card_scripts;
mod guess;
mod scripts;
mod submit;

use card_scripts::CARD_JS;
use guess::GUESS_JS;
use scripts::{CLASSIFY_JS, FILL_JS, FOCUS_JS, RELEASE_JS};
use submit::SUBMIT_JS;

/// Chromium's address patterns, which `GUESS_JS` reads.
const PATTERNS_JS: &str = include_str!("patterns.js");

const FIELD_EVENT: &str = "wis-address-field";
const CARD_FIELD_EVENT: &str = "wis-card-field";
const MOVE_EVENT: &str = "wis-address-move";
const SCROLL_EVENT: &str = "wis-address-scroll";
const LEAVE_EVENT: &str = "wis-address-leave";

/// The kinds of fields `CLASSIFY_JS` knows.
const KINDS: [&str; 14] = [
    "name",
    "given",
    "family",
    "organization",
    "street",
    "line1",
    "line2",
    "line3",
    "city",
    "region",
    "postal",
    "country",
    "tel",
    "email",
];
/// The kinds of card fields `CARD_JS` knows.
const CARD_KINDS: [&str; 8] = [
    "number", "name", "given", "family", "exp", "month", "year", "type",
];
const FOCUS_INTERVAL: Duration = Duration::from_millis(250);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Form {
    Address,
    Card,
}

/// What a tab's last field report offered: the origin it came from, whether
/// an address or a card, the kinds of fields a fill would reach, and the
/// token naming that report.
#[derive(Clone)]
struct Offered {
    origin: String,
    form: Form,
    kinds: Vec<&'static str>,
    token: String,
}

static OFFERED: Mutex<Option<HashMap<String, Offered>>> = Mutex::new(None);
static LAST_FOCUS: Mutex<Option<HashMap<String, Instant>>> = Mutex::new(None);

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct FieldEvent<'a> {
    window: String,
    tab_id: &'a str,
    host: String,
    rect: FieldRect,
    kinds: Vec<&'static str>,
    token: &'a str,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct MoveEvent<'a> {
    tab_id: &'a str,
    rect: FieldRect,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct TabEvent<'a> {
    tab_id: &'a str,
}

pub fn page_script() -> &'static str {
    static SCRIPT: LazyLock<String> = LazyLock::new(|| {
        format!(
            "(function () {{\nif (window.__wisAddressInstalled) return;\nwindow.__wisAddressInstalled = true;\n{PATTERNS_JS}\n{GUESS_JS}\n{CLASSIFY_JS}\n{CARD_JS}\n{FOCUS_JS}\n{SUBMIT_JS}\n}})();"
        )
    });
    &SCRIPT
}

/// The kinds of `known` a report lists, in order and once each.
fn known_kinds(listed: Option<&serde_json::Value>, known: &[&'static str]) -> Vec<&'static str> {
    let mut kinds = Vec::new();
    for value in listed
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
    {
        let known = value
            .as_str()
            .and_then(|kind| known.iter().find(|known| **known == kind));
        if let Some(kind) = known {
            if !kinds.contains(kind) {
                kinds.push(*kind);
            }
        }
    }
    kinds
}

/// Whether a report's token is one the page script makes: 32 hex digits.
fn valid_token(token: &str) -> bool {
    token.len() == 32
        && token
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn focus_allowed(tab_id: &str, now: Instant) -> bool {
    let mut last = LAST_FOCUS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let last = last.get_or_insert_with(HashMap::new);
    if last
        .get(tab_id)
        .is_some_and(|at| now.saturating_duration_since(*at) < FOCUS_INTERVAL)
    {
        return false;
    }
    last.insert(tab_id.to_string(), now);
    true
}

/// Forward one report from a page's address script to the chrome. Shared by
/// all three platforms; the tab, window and origin are read from the webview.
pub(crate) fn dispatch_message(label: &str, body: &serde_json::Value) {
    let Some(from) = source(label) else {
        return;
    };
    let number = |key: &str| body.get(key).and_then(serde_json::Value::as_f64);
    let rect = || {
        field_rect(
            number("x"),
            number("y"),
            number("width"),
            number("height"),
            number("viewWidth"),
        )
    };
    let emitted = match body.get("kind").and_then(serde_json::Value::as_str) {
        Some("focus") => {
            let form = match body.get("form").and_then(serde_json::Value::as_str) {
                Some("card") => Form::Card,
                _ => Form::Address,
            };
            let kinds = known_kinds(
                body.get("kinds"),
                if form == Form::Card {
                    &CARD_KINDS
                } else {
                    &KINDS
                },
            );
            let token = body.get("token").and_then(serde_json::Value::as_str);
            let (Some(rect), Some(origin), Some(token)) = (
                rect(),
                origin_of(from.app, label),
                token.filter(|token| valid_token(token)),
            ) else {
                return;
            };
            if kinds.is_empty()
                || (form == Form::Card && !card_fill::card_origin(&origin))
                || !focus_allowed(from.tab_id, Instant::now())
            {
                return;
            }

            remember(
                from.tab_id,
                Offered {
                    origin,
                    form,
                    kinds: kinds.clone(),
                    token: token.to_string(),
                },
            );
            from.app.emit(
                if form == Form::Card {
                    CARD_FIELD_EVENT
                } else {
                    FIELD_EVENT
                },
                FieldEvent {
                    window: from.window,
                    tab_id: from.tab_id,
                    host: from.host,
                    rect,
                    kinds,
                    token,
                },
            )
        }
        Some("move") => {
            let Some(rect) = rect() else {
                return;
            };
            from.app.emit(
                MOVE_EVENT,
                MoveEvent {
                    tab_id: from.tab_id,
                    rect,
                },
            )
        }
        Some("scroll") => from.app.emit(
            SCROLL_EVENT,
            TabEvent {
                tab_id: from.tab_id,
            },
        ),
        Some("save") => {
            if origin_of(from.app, label).is_some() {
                crate::vault::address_save::receive(from.app, from.tab_id, body.get("values"));
            }
            return;
        }
        Some("card") => {
            if origin_of(from.app, label).is_some_and(|origin| card_fill::card_origin(&origin)) {
                crate::vault::card_save::receive(from.app, from.tab_id, body.get("values"));
            }
            return;
        }
        Some("leave") => from.app.emit(
            LEAVE_EVENT,
            TabEvent {
                tab_id: from.tab_id,
            },
        ),
        _ => return,
    };
    if let Err(err) = emitted {
        log::warn!("address field event failed: {err}");
    }
}

/// The origin of the page in `label`.
fn origin_of(app: &AppHandle, label: &str) -> Option<String> {
    let origin = app.webviews().get(label)?.url().ok()?.origin();
    origin.is_tuple().then(|| origin.ascii_serialization())
}

fn remember(tab_id: &str, offered: Offered) {
    OFFERED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get_or_insert_with(HashMap::new)
        .insert(tab_id.to_string(), offered);
}

/// The tab's offer, if it is still the report `token` names and for `form`.
fn take(tab_id: &str, token: &str, form: Form) -> Option<Offered> {
    let mut offered = OFFERED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let offered = offered.as_mut()?;
    let current = offered.get(tab_id)?;
    if current.token != token || current.form != form {
        return None;
    }
    offered.remove(tab_id)
}

/// The origin of the tab's offer, while it is still the report `token`
/// names, for `form`.
fn current(tab_id: &str, token: &str, form: Form) -> Option<String> {
    OFFERED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .as_ref()
        .and_then(|offered| offered.get(tab_id))
        .filter(|offered| offered.token == token && offered.form == form)
        .map(|offered| offered.origin.clone())
}

pub(crate) fn forget(tab_id: &str) {
    if let Some(offered) = OFFERED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .as_mut()
    {
        offered.remove(tab_id);
    }
    if let Some(last) = LAST_FOCUS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .as_mut()
    {
        last.remove(tab_id);
    }
}

/// Fill saved address `id` into the address field report `token` names, if
/// it is still the tab's last and the page is still on the origin it came
/// from, with only the kinds of values that report said a fill would reach.
/// Runs in Wis's own script world.
#[tauri::command]
pub async fn address_fill(
    app: AppHandle,
    state: State<'_, AppState>,
    tab_id: TabId,
    id: i64,
    token: String,
) -> WisResult<()> {
    if !state.browser.lock().await.settings.address_autofill {
        return Err(invalid("Filling addresses is turned off"));
    }

    let tab = tab_id.as_uuid().to_string();
    let label = format!("tab-{tab}");
    let address = crate::vault::addresses::opened(&state, id).await?;
    let offered =
        take(&tab, &token, Form::Address).ok_or_else(|| invalid("The page has moved on"))?;
    let webview = app
        .webviews()
        .get(&label)
        .cloned()
        .ok_or_else(|| invalid("That tab is gone"))?;
    if origin_of(&app, &label).as_deref() != Some(offered.origin.as_str()) {
        return Err(invalid("The page has moved on"));
    }

    let script = fill_script(&offered, &address);
    crate::platform::content_world::eval(&webview, script).map_err(|err| WisError::InvalidInput {
        message: format!("address fill failed: {err}"),
    })
}

/// Stop the page reporting where its address field is, once the card closes.
#[tauri::command]
pub fn release_address_field(app: AppHandle, tab_id: TabId) {
    let tab = tab_id.as_uuid().to_string();
    forget(&tab);
    if let Some(webview) = app.webviews().get(&format!("tab-{tab}")) {
        if let Err(err) = crate::platform::content_world::eval(webview, RELEASE_JS) {
            log::warn!("release address field failed: {err}");
        }
    }
}

/// The fill for `offered`: its origin and token, and only the values its
/// kinds reach.
fn fill_script(offered: &Offered, address: &wis_core::vault::address::Address) -> String {
    let reaches = |kinds: &[&str]| kinds.iter().any(|kind| offered.kinds.contains(kind));
    fn keep(reached: bool, value: &str) -> &str {
        if reached {
            value
        } else {
            ""
        }
    }

    let values = serde_json::json!({
        "given": keep(reaches(&["name", "given"]), &address.given_name),
        "family": keep(reaches(&["name", "family"]), &address.family_name),
        "organization": keep(reaches(&["organization"]), &address.organization),
        "street": keep(reaches(&["street", "line1", "line2", "line3"]), &address.street_address),
        "city": keep(reaches(&["city"]), &address.city),
        "region": keep(reaches(&["region"]), &address.region),
        "postal": keep(reaches(&["postal"]), &address.postal_code),
        "country": keep(reaches(&["country"]), &address.country),
        "tel": keep(reaches(&["tel"]), &address.tel),
        "email": keep(reaches(&["email"]), &address.email),
    });
    let origin = serde_json::Value::String(offered.origin.clone());
    let token = serde_json::Value::String(offered.token.clone());
    format!(
        "(function () {{\nvar ORIGIN = {origin};\nvar TOKEN = {token};\nvar V = {values};\n{PATTERNS_JS}\n{GUESS_JS}\n{CLASSIFY_JS}\n{FILL_JS}\n}})();"
    )
}

fn invalid(message: &str) -> WisError {
    WisError::InvalidInput {
        message: message.to_string(),
    }
}

/// Install the address reporter and its `wisAddress` handler on a tab
/// webview. macOS only; Windows and Linux route through their dispatchers.
pub fn install<R: tauri::Runtime>(webview: &tauri::webview::Webview<R>) {
    #[cfg(target_os = "macos")]
    macos::install(webview, webview.label().to_string());

    #[cfg(not(target_os = "macos"))]
    let _ = webview;
}

#[cfg(target_os = "macos")]
mod macos {
    use std::ffi::CStr;
    use std::os::raw::c_char;

    use objc2::rc::Id;
    use objc2::runtime::{AnyObject, NSObject};
    use objc2::{
        class, declare_class, msg_send, msg_send_id, mutability, ClassType, DeclaredClass,
    };

    use crate::platform::content_world::macos as content_world;

    use super::dispatch_message;

    struct HandlerIvars {
        label: String,
    }

    declare_class!(
        struct AddressHandler;

        unsafe impl ClassType for AddressHandler {
            type Super = NSObject;
            type Mutability = mutability::InteriorMutable;
            const NAME: &'static str = "WisAddressHandler";
        }

        impl DeclaredClass for AddressHandler {
            type Ivars = HandlerIvars;
        }

        unsafe impl AddressHandler {
            #[method(userContentController:didReceiveScriptMessage:)]
            fn did_receive(&self, _ucc: *mut AnyObject, message: *mut AnyObject) {
                let label = self.ivars().label.clone();
                unsafe { handle_message(&label, message) }
            }
        }
    );

    impl AddressHandler {
        fn new(label: String) -> Id<Self> {
            let this = Self::alloc().set_ivars(HandlerIvars { label });
            unsafe { msg_send_id![super(this), init] }
        }
    }

    unsafe fn nsstring_to_string(ns: *mut AnyObject) -> Option<String> {
        if ns.is_null() {
            return None;
        }
        let is_str: bool = msg_send![ns, isKindOfClass: class!(NSString)];
        if !is_str {
            return None;
        }
        let utf8: *const c_char = msg_send![ns, UTF8String];
        if utf8.is_null() {
            return None;
        }
        Some(CStr::from_ptr(utf8).to_string_lossy().into_owned())
    }

    unsafe fn handle_message(label: &str, message: *mut AnyObject) {
        if message.is_null() {
            return;
        }
        let frame: *mut AnyObject = msg_send![message, frameInfo];
        if frame.is_null() {
            return;
        }
        let is_main_frame: bool = msg_send![frame, isMainFrame];
        if !is_main_frame {
            return;
        }
        let body: *mut AnyObject = msg_send![message, body];
        let Some(raw) = nsstring_to_string(body) else {
            return;
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&raw) else {
            return;
        };
        dispatch_message(label, &value);
    }

    pub(super) fn install<R: tauri::Runtime>(webview: &tauri::webview::Webview<R>, label: String) {
        let script = super::page_script();
        let installed = webview.with_webview(move |w| unsafe {
            let wk: *mut AnyObject = w.inner() as *mut AnyObject;
            if wk.is_null() {
                return;
            }
            let config: *mut AnyObject = msg_send![wk, configuration];
            if config.is_null() {
                return;
            }
            let controller: *mut AnyObject = msg_send![config, userContentController];
            if controller.is_null() {
                return;
            }
            let handler = AddressHandler::new(label.clone());
            content_world::add_handler(controller, &*handler, "wisAddress");
            content_world::install_script(wk, controller, &label, "wisAddress", script, true);
        });
        if let Err(err) = installed {
            log::warn!("address field script not installed: {err}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOKEN: &str = "0123456789abcdef0123456789abcdef";
    const OTHER: &str = "fedcba9876543210fedcba9876543210";

    fn home() -> wis_core::vault::address::Address {
        wis_core::vault::address::Address {
            given_name: "Ada".into(),
            family_name: "Lovelace".into(),
            street_address: "12 St James's Square\nFlat \"3\"".into(),
            city: "London".into(),
            postal_code: "SW1Y 4JH".into(),
            tel: "+44 20 7946 0000".into(),
            email: "ada@example.org".into(),
            ..Default::default()
        }
    }

    #[test]
    fn a_fill_carries_only_the_values_its_fields_reach() {
        let offered = Offered {
            origin: "https://shop.example".into(),
            form: Form::Address,
            kinds: vec!["postal", "city"],
            token: TOKEN.into(),
        };
        let script = fill_script(&offered, &home());
        assert!(script.contains("var ORIGIN = \"https://shop.example\";"));
        assert!(script.contains(&format!("var TOKEN = \"{TOKEN}\";")));
        assert!(script.contains("\"postal\":\"SW1Y 4JH\""));
        assert!(script.contains("\"city\":\"London\""));
        for kept_out in ["Ada", "Lovelace", "7946", "ada@example.org", "James"] {
            assert!(!script.contains(kept_out), "{kept_out}");
        }
    }

    #[test]
    fn values_are_quoted_so_they_cannot_end_the_script() {
        let offered = Offered {
            origin: "https://shop.example".into(),
            form: Form::Address,
            kinds: vec!["street"],
            token: TOKEN.into(),
        };
        let script = fill_script(&offered, &home());
        assert!(script.contains(r#""street":"12 St James's Square\nFlat \"3\"""#));
    }

    #[test]
    fn a_fill_takes_only_the_offer_the_card_showed() {
        let tab = "tab-test-take";
        let offer = |token: &str| Offered {
            origin: "https://shop.example".into(),
            form: Form::Address,
            kinds: vec!["postal"],
            token: token.into(),
        };
        remember(tab, offer(TOKEN));
        remember(tab, offer(OTHER));
        assert!(
            take(tab, TOKEN, Form::Address).is_none(),
            "a newer report came in"
        );
        assert_eq!(
            current(tab, OTHER, Form::Address).as_deref(),
            Some("https://shop.example")
        );
        assert!(current(tab, OTHER, Form::Card).is_none());
        assert_eq!(
            take(tab, OTHER, Form::Address).map(|offered| offered.token),
            Some(OTHER.into())
        );
        assert!(
            take(tab, OTHER, Form::Address).is_none(),
            "an offer fills once"
        );
    }

    #[test]
    fn a_token_is_32_lowercase_hex_digits() {
        assert!(valid_token(TOKEN));
        for bad in [
            "",
            "0123",
            &TOKEN.to_uppercase(),
            &format!("{TOKEN}0"),
            "zz0123456789abcdef0123456789abcd",
        ] {
            assert!(!valid_token(bad), "{bad}");
        }
    }

    #[test]
    fn forgetting_a_tab_lets_it_report_again() {
        let at = Instant::now();
        assert!(focus_allowed("tab-test-forget", at));
        forget("tab-test-forget");
        assert!(focus_allowed("tab-test-forget", at));
    }

    #[test]
    fn the_page_and_fill_scripts_carry_the_patterns_and_the_guessing() {
        let offered = Offered {
            origin: "https://shop.example".into(),
            form: Form::Address,
            kinds: vec!["postal"],
            token: TOKEN.into(),
        };
        for script in [page_script(), fill_script(&offered, &home()).as_str()] {
            let at = |needle: &str| script.find(needle).expect(needle);
            assert!(at("function addressPatterns()") < at("var GUESS_ORDER"));
            assert!(at("var GUESS_ORDER") < at("function addressFields("));
        }
        let page = page_script();
        assert!(page.find("function send(").unwrap() < page.find("function offerToSave(").unwrap());
        assert!(
            page.find("function cardOfferKinds(").unwrap() < page.find("function report(").unwrap()
        );
    }

    #[test]
    fn a_report_lists_only_known_kinds_once_each() {
        let listed = serde_json::json!(["postal", "city", "postal", "password", 7, "email"]);
        assert_eq!(
            known_kinds(Some(&listed), &KINDS),
            vec!["postal", "city", "email"]
        );
        assert!(known_kinds(None, &KINDS).is_empty());
        assert!(known_kinds(Some(&serde_json::json!("postal")), &KINDS).is_empty());
        let card = serde_json::json!(["number", "postal", "exp", "csc"]);
        assert_eq!(known_kinds(Some(&card), &CARD_KINDS), vec!["number", "exp"]);
    }

    #[test]
    fn a_tab_may_report_a_focus_four_times_a_second_at_most() {
        let at = Instant::now();
        assert!(focus_allowed("tab-test-throttle", at));
        assert!(!focus_allowed(
            "tab-test-throttle",
            at + Duration::from_millis(100)
        ));
        assert!(focus_allowed(
            "tab-test-throttle",
            at + Duration::from_millis(300)
        ));
        assert!(focus_allowed(
            "tab-other-throttle",
            at + Duration::from_millis(100)
        ));
    }
}

//! "Save address?" after a page submits an address the user typed. The page
//! script reports the shown address fields' values on a trusted submit; the
//! address waits here, one per tab, until the user saves or dismisses it.
//! Private workspaces and the setting being off drop it on arrival.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use wis_core::vault::address::Address;
use wis_core::{TabId, WisError, WisResult};

use crate::state::AppState;

const CAPTURE_EVENT: &str = "wis-address-capture";
const OFFER_INTERVAL: Duration = Duration::from_secs(1);
const QUIET_FOR: Duration = Duration::from_secs(60);

static LAST_OFFER: Mutex<Option<HashMap<TabId, Instant>>> = Mutex::new(None);
static DISMISSED: Mutex<Option<HashMap<TabId, Instant>>> = Mutex::new(None);
static SERIAL: AtomicU64 = AtomicU64::new(1);

/// A submitted address, with the code of a region picked from a list, the
/// name as one field when the page had only one, a serial naming this
/// capture for its prompt, and whether its prompt waits to be opened.
#[derive(Clone)]
pub struct Captured {
    address: Address,
    region_code: String,
    whole_name: String,
    serial: u64,
    quiet: bool,
}

/// What saving an address shows: the address, the serial a save names,
/// and whether to wait to be opened.
#[derive(Clone, Serialize)]
pub struct PendingAddress {
    serial: u64,
    address: Address,
    quiet: bool,
}

#[derive(Clone, Serialize)]
struct CaptureEvent<'a> {
    tab: TabId,
    serial: u64,
    address: &'a Address,
    quiet: bool,
}

/// Take an address the page in tab `tab_id` reported as submitted.
pub(crate) fn receive(app: &AppHandle, tab_id: &str, values: Option<&serde_json::Value>) {
    let Some(captured) = captured(values) else {
        return;
    };
    let Ok(uuid) = uuid::Uuid::parse_str(tab_id) else {
        return;
    };
    let tab = TabId::from_uuid(uuid);
    if !offer_allowed(tab, Instant::now()) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        intake(app, tab, captured).await;
    });
}

/// Hold `captured` for `tab` and offer it, unless saving is off. An address
/// already saved is also offered.
async fn intake(app: AppHandle, tab: TabId, mut captured: Captured) {
    let state = app.state::<AppState>();
    if !may_save(&state, tab).await {
        return;
    }

    captured.quiet = hushed(tab, Instant::now());
    // Held only for tabs still open, as one may have closed meanwhile.
    {
        let browser = state.browser.lock().await;
        let mut pending = state.pending_addresses.lock().await;
        pending.retain(|id, _| browser.tab(*id).is_ok());
        let same = pending.get(&tab).is_some_and(|held| {
            held.address == captured.address && held.region_code == captured.region_code
        });
        if same || browser.tab(tab).is_err() {
            return;
        }
        pending.insert(tab, captured.clone());
    }

    let event = CaptureEvent {
        tab,
        serial: captured.serial,
        address: &captured.address,
        quiet: captured.quiet,
    };
    if let Err(err) = app.emit(CAPTURE_EVENT, event) {
        log::warn!("address capture event failed: {err}");
    }
}

/// Whether `tab` may offer to save an address: the setting is on and the tab
/// is not in a private workspace.
async fn may_save(state: &AppState, tab: TabId) -> bool {
    let browser = state.browser.lock().await;
    browser.settings.address_autofill
        && browser
            .tab(tab)
            .and_then(|tab| browser.workspace(tab.workspace_id))
            .is_ok_and(|workspace| !workspace.ephemeral)
}

/// `tab_id`'s submitted address, for a surface that opens after the event.
#[tauri::command]
pub async fn address_capture_pending(
    state: State<'_, AppState>,
    tab_id: TabId,
) -> WisResult<Option<PendingAddress>> {
    let pending = state.pending_addresses.lock().await;
    Ok(pending.get(&tab_id).map(|captured| PendingAddress {
        serial: captured.serial,
        address: captured.address.clone(),
        quiet: captured.quiet,
    }))
}

/// Save the address `serial` names, the one the prompt showed for `tab_id`,
/// unless a saved one already holds it. Returns whether it was saved. Needs
/// the vault unlocked.
#[tauri::command]
pub async fn address_capture_save(
    state: State<'_, AppState>,
    tab_id: TabId,
    serial: u64,
) -> WisResult<bool> {
    let captured =
        waiting(&*state.pending_addresses.lock().await, tab_id, serial).ok_or_else(|| {
            WisError::InvalidInput {
                message: "That address is no longer waiting to be saved".to_string(),
            }
        })?;
    let address = captured.address.clone();
    let saved = crate::vault::addresses::add_unless(&state, address, |known| {
        already_saved(known, &captured)
    })
    .await?;
    drop_shown(&mut *state.pending_addresses.lock().await, tab_id, serial);
    Ok(saved)
}

/// Drop `tab`'s waiting address if it is still the one `serial` names.
fn drop_shown(pending: &mut HashMap<TabId, Captured>, tab: TabId, serial: u64) {
    if pending.get(&tab).is_some_and(|held| held.serial == serial) {
        pending.remove(&tab);
    }
}

/// `tab`'s waiting address, if it is still the one `serial` names.
fn waiting(pending: &HashMap<TabId, Captured>, tab: TabId, serial: u64) -> Option<Captured> {
    pending
        .get(&tab)
        .filter(|captured| captured.serial == serial)
        .cloned()
}

/// Drop the address `serial` names, the one the prompt showed for `tab_id`.
/// The tab's next prompt waits a minute to be opened, so a page cannot keep
/// putting one in front of the user.
#[tauri::command]
pub async fn address_capture_dismiss(
    state: State<'_, AppState>,
    tab_id: TabId,
    serial: u64,
) -> WisResult<()> {
    drop_shown(&mut *state.pending_addresses.lock().await, tab_id, serial);
    hush(tab_id, Instant::now());
    Ok(())
}

/// The prompt for `tab_id` was closed without an answer: its address waits,
/// and the next one waits a minute to be opened.
#[tauri::command]
pub fn address_capture_hush(tab_id: TabId) {
    hush(tab_id, Instant::now());
}

/// Drop a tab's submitted address, as when it closes.
pub async fn forget(state: &AppState, tab: TabId) {
    state.pending_addresses.lock().await.remove(&tab);
    if let Some(dismissed) = DISMISSED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .as_mut()
    {
        dismissed.remove(&tab);
    }
}

fn hush(tab: TabId, now: Instant) {
    DISMISSED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get_or_insert_with(HashMap::new)
        .insert(tab, now);
}

/// Whether `tab` had an offer dismissed in the last minute.
fn hushed(tab: TabId, now: Instant) -> bool {
    let mut dismissed = DISMISSED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let dismissed = dismissed.get_or_insert_with(HashMap::new);
    dismissed.retain(|_, at| now.saturating_duration_since(*at) < QUIET_FOR);
    dismissed.contains_key(&tab)
}

/// The address in a page's report: its known values, a full name split into
/// given and family names when those are missing, and a country only as a
/// code. None unless it holds a street and a city or postal code.
fn captured(values: Option<&serde_json::Value>) -> Option<Captured> {
    let values = values?.as_object()?;
    let text = |key: &str| {
        values
            .get(key)
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string()
    };
    let (mut given, mut family) = (text("given"), text("family"));
    let mut whole_name = String::new();
    if given.trim().is_empty() && family.trim().is_empty() {
        whole_name = text("name").trim().to_string();
        (given, family) = split_name(&whole_name);
    }

    let country = text("country");
    let code = country.trim().len() == 2 && country.trim().bytes().all(|b| b.is_ascii_alphabetic());
    let address = Address {
        given_name: given,
        family_name: family,
        organization: text("organization"),
        street_address: text("street"),
        city: text("city"),
        region: text("region"),
        postal_code: text("postal"),
        country: if code { country } else { String::new() },
        tel: text("tel"),
        email: text("email"),
    }
    .tidied()
    .ok()?;
    let placed = !address.city.is_empty() || !address.postal_code.is_empty();
    if address.street_address.is_empty() || !placed {
        return None;
    }

    let short = |text: String| {
        if text.chars().count() <= wis_core::vault::address::MAX_FIELD_CHARS {
            text
        } else {
            String::new()
        }
    };
    Some(Captured {
        address,
        region_code: short(text("regionCode").trim().to_string()),
        whole_name: short(whole_name),
        serial: SERIAL.fetch_add(1, Ordering::Relaxed),
        quiet: false,
    })
}

/// A full name as given and family names: the last word is the family name,
/// but the first in Chinese, Japanese or Korean.
fn split_name(name: &str) -> (String, String) {
    let name = name.trim();
    if name.chars().any(family_first) {
        return match name.split_once(char::is_whitespace) {
            Some((family, given)) => (given.trim().to_string(), family.to_string()),
            None => (name.to_string(), String::new()),
        };
    }
    match name.rsplit_once(char::is_whitespace) {
        Some((given, family)) => (given.trim().to_string(), family.to_string()),
        None => (name.to_string(), String::new()),
    }
}

fn family_first(c: char) -> bool {
    matches!(c,
        '\u{1100}'..='\u{11ff}'
            | '\u{3040}'..='\u{30ff}'
            | '\u{3130}'..='\u{318f}'
            | '\u{31f0}'..='\u{31ff}'
            | '\u{3400}'..='\u{4dbf}'
            | '\u{4e00}'..='\u{9fff}'
            | '\u{ac00}'..='\u{d7af}'
            | '\u{f900}'..='\u{faff}'
            | '\u{ff66}'..='\u{ff9f}'
            | '\u{20000}'..='\u{323af}')
}

/// Whether a saved address in `known` already holds `captured`, its region
/// matching by name or by the code picked from a list, and a name given whole
/// matching the given and family names together.
fn already_saved(known: &[Address], captured: &Captured) -> bool {
    let whole = !captured.whole_name.is_empty();
    let unnamed = |region: String| Address {
        region,
        given_name: if whole {
            String::new()
        } else {
            captured.address.given_name.clone()
        },
        family_name: if whole {
            String::new()
        } else {
            captured.address.family_name.clone()
        },
        ..captured.address.clone()
    };
    let by_name = unnamed(captured.address.region.clone());
    let by_code = unnamed(String::new());
    let code = Address {
        region: captured.region_code.clone(),
        ..Address::default()
    };
    known.iter().any(|saved| {
        let coded = !captured.region_code.is_empty() && saved.covers(&code);
        let named = !whole || saved.named(&captured.whole_name);
        named && saved.covers(if coded { &by_code } else { &by_name })
    })
}

/// Whether `tab` may offer an address now: once a second at most.
fn offer_allowed(tab: TabId, now: Instant) -> bool {
    let mut last = LAST_OFFER
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let last = last.get_or_insert_with(HashMap::new);
    last.retain(|_, at| now.saturating_duration_since(*at) < OFFER_INTERVAL);
    if last
        .get(&tab)
        .is_some_and(|at| now.saturating_duration_since(*at) < OFFER_INTERVAL)
    {
        return false;
    }
    last.insert(tab, now);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn submitted(values: serde_json::Value) -> Option<Address> {
        captured(Some(&values)).map(|captured| captured.address)
    }

    #[test]
    fn a_submitted_address_takes_known_values_and_splits_a_full_name() {
        let address = submitted(serde_json::json!({
            "name": " Ada Augusta  Lovelace ",
            "street": "12 St James Square\nFlat 3",
            "city": "London",
            "postal": "SW1Y 4JH",
            "country": "gb",
            "tel": 7,
            "password": "hunter2",
        }))
        .unwrap();
        assert_eq!(address.given_name, "Ada Augusta");
        assert_eq!(address.family_name, "Lovelace");
        assert_eq!(address.street_address, "12 St James Square\nFlat 3");
        assert_eq!(address.country, "GB");
        assert_eq!(address.tel, "");

        let given = submitted(serde_json::json!({
            "given": "Ada", "name": "Someone Else", "street": "1 Road", "city": "Leeds",
        }))
        .unwrap();
        assert_eq!(
            (given.given_name.as_str(), given.family_name.as_str()),
            ("Ada", "")
        );
        let single =
            submitted(serde_json::json!({ "name": "山田太郎", "street": "1", "postal": "100" }));
        assert_eq!(single.unwrap().given_name, "山田太郎");
    }

    #[test]
    fn only_a_street_with_a_city_or_postcode_is_an_address() {
        assert!(submitted(serde_json::json!({ "name": "Ada", "city": "London" })).is_none());
        assert!(submitted(serde_json::json!({ "street": "1 Road" })).is_none());
        assert!(submitted(serde_json::json!({ "street": "1 Road", "postal": "LS1" })).is_some());
        assert!(submitted(serde_json::json!("1 Road")).is_none());
        assert!(captured(None).is_none());
    }

    #[test]
    fn a_country_that_is_not_a_code_is_left_out_and_an_oversized_field_refused() {
        let named = submitted(serde_json::json!({
            "street": "1 Road", "city": "Leeds", "country": "United Kingdom",
        }))
        .unwrap();
        assert_eq!(named.country, "");
        let huge = "x".repeat(10_000);
        assert!(submitted(serde_json::json!({ "street": "1 Road", "city": huge })).is_none());
    }

    #[test]
    fn an_address_already_saved_is_not_offered_again_by_its_values_or_region_code() {
        let saved = Address {
            given_name: "Ada".into(),
            family_name: "Lovelace".into(),
            street_address: "1 Main Street\nApt 2".into(),
            city: "Albany".into(),
            region: "NY".into(),
            postal_code: "12207".into(),
            country: "US".into(),
            ..Address::default()
        };
        let typed = captured(Some(&serde_json::json!({
            "name": "Ada Lovelace",
            "street": "1 Main Street, Apt 2",
            "city": "albany",
            "region": "New York",
            "regionCode": "NY",
            "postal": "12207",
            "country": "US",
        })))
        .unwrap();
        assert!(already_saved(std::slice::from_ref(&saved), &typed));

        let moved = captured(Some(&serde_json::json!({
            "street": "9 Elm Road", "city": "Albany", "postal": "12207",
        })))
        .unwrap();
        assert!(!already_saved(&[saved], &moved));
    }

    #[test]
    fn a_whole_name_matches_whole_and_a_family_first_name_splits_that_way() {
        let saved = Address {
            given_name: "Juan".into(),
            family_name: "García López".into(),
            street_address: "Calle Mayor 1".into(),
            city: "Madrid".into(),
            ..Address::default()
        };
        let typed = captured(Some(&serde_json::json!({
            "name": "Juan García López", "street": "Calle Mayor 1", "city": "Madrid",
        })))
        .unwrap();
        assert!(already_saved(&[saved], &typed));

        let japanese =
            submitted(serde_json::json!({ "name": "山田 太郎", "street": "1", "postal": "100" }))
                .unwrap();
        assert_eq!(
            (japanese.given_name.as_str(), japanese.family_name.as_str()),
            ("太郎", "山田")
        );
    }

    #[test]
    fn every_capture_counts_as_saved_once_it_is() {
        for values in [
            serde_json::json!({ "name": "山田 太郎", "street": "1-1", "postal": "100-0001" }),
            serde_json::json!({ "name": "김 민준", "street": "1", "city": "Seoul" }),
            serde_json::json!({ "name": "Juan García López", "street": "1", "city": "Madrid" }),
            serde_json::json!({
                "street": "1 Main St", "city": "Albany", "region": "New York", "regionCode": "NY",
            }),
        ] {
            let typed = captured(Some(&values)).unwrap();
            assert!(
                already_saved(std::slice::from_ref(&typed.address), &typed),
                "{values}"
            );
        }
        assert!(family_first('ｶ') && family_first('ㄱ') && family_first('\u{20000}'));
    }

    #[test]
    fn a_save_takes_only_the_address_the_menu_showed() {
        let tab = TabId::new();
        let shown = captured(Some(
            &serde_json::json!({ "street": "1 Road", "city": "Leeds" }),
        ))
        .unwrap();
        let pending = HashMap::from([(tab, shown.clone())]);
        assert!(waiting(&pending, tab, shown.serial).is_some());
        assert!(waiting(&pending, tab, shown.serial + 1).is_none());
        assert!(waiting(&pending, TabId::new(), shown.serial).is_none());

        let mut held = pending.clone();
        drop_shown(&mut held, tab, shown.serial + 1);
        assert!(
            held.contains_key(&tab),
            "a newer address than the one dismissed"
        );
        drop_shown(&mut held, tab, shown.serial);
        assert!(held.is_empty());
    }

    #[test]
    fn a_tab_whose_offer_was_dismissed_stays_quiet_for_a_minute() {
        let tab = TabId::new();
        let at = Instant::now();
        assert!(!hushed(tab, at));
        hush(tab, at);
        assert!(hushed(tab, at + Duration::from_secs(59)));
        assert!(!hushed(tab, at + Duration::from_secs(61)));
        assert!(!hushed(TabId::new(), at));
    }

    #[test]
    fn a_tab_may_offer_an_address_once_a_second() {
        let tab = TabId::new();
        let at = Instant::now();
        assert!(offer_allowed(tab, at));
        assert!(!offer_allowed(tab, at + Duration::from_millis(500)));
        assert!(offer_allowed(tab, at + Duration::from_millis(1100)));
        assert!(offer_allowed(TabId::new(), at + Duration::from_millis(500)));
    }
}

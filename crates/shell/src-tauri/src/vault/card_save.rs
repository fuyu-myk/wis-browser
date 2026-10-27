//! Card saving prompt after a page submits a card number the user typed. The
//! page script reports the card fields' values on a trusted submit, never the
//! security code. The card waits in memory, one per tab, until the user saves
//! or dismisses it. Only https pages with the setting on, and never private
//! workspaces.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use wis_core::vault::card::{Card, CardSummary};
use wis_core::{TabId, WisError, WisResult};
use zeroize::Zeroizing;

use crate::state::AppState;
use crate::vault::cards::CardSaved;

const CAPTURE_EVENT: &str = "wis-card-capture";
const OFFER_INTERVAL: Duration = Duration::from_secs(1);
const QUIET_FOR: Duration = Duration::from_secs(60);

static LAST_OFFER: Mutex<Option<HashMap<TabId, Instant>>> = Mutex::new(None);
static DISMISSED: Mutex<Option<HashMap<TabId, Instant>>> = Mutex::new(None);
static SERIAL: AtomicU64 = AtomicU64::new(1);

/// A submitted card, a serial naming this capture for its prompt, and whether
/// its prompt waits to be opened. The number is wiped when it is dropped.
#[derive(Clone)]
pub struct Captured {
    card: Card,
    serial: u64,
    quiet: bool,
}

/// What saving a card shows: the full number is never shown.
#[derive(Clone, Serialize)]
pub struct PendingCard {
    serial: u64,
    #[serde(flatten)]
    summary: CardSummary,
    name: String,
    quiet: bool,
}

#[derive(Clone, Serialize)]
struct CaptureEvent {
    tab: TabId,
    serial: u64,
    quiet: bool,
}

/// Take a card the page in tab `tab_id` reported as submitted.
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

/// Hold `captured` for `tab` and offer it, unless saving is off or the exact
/// card is already saved.
async fn intake(app: AppHandle, tab: TabId, mut captured: Captured) {
    let state = app.state::<AppState>();
    if !may_save(&state, tab).await {
        return;
    }

    let storage = state.storage.clone();
    let listed = crate::vault::store::blocking(move || {
        storage
            .list_cards()
            .map_err(|err| WisError::PersistenceFailed {
                reason: err.to_string(),
            })
    })
    .await;
    let Ok(listed) = listed else {
        return;
    };
    let summaries: Vec<CardSummary> = listed.iter().map(crate::vault::cards::summary_of).collect();
    if already_listed(&summaries, &captured.card) {
        return;
    }

    captured.quiet = hushed(tab, Instant::now());
    {
        let browser = state.browser.lock().await;
        let mut pending = state.pending_cards.lock().await;
        pending.retain(|id, _| browser.tab(*id).is_ok());
        let same = pending
            .get(&tab)
            .is_some_and(|held| held.card == captured.card);
        if same || browser.tab(tab).is_err() {
            return;
        }
        pending.insert(tab, captured.clone());
    }

    let event = CaptureEvent {
        tab,
        serial: captured.serial,
        quiet: captured.quiet,
    };
    if let Err(err) = app.emit(CAPTURE_EVENT, event) {
        log::warn!("card capture event failed: {err}");
    }
}

/// Whether `tab` may offer to save a card: the setting is on and the tab is
/// not in a private workspace.
async fn may_save(state: &AppState, tab: TabId) -> bool {
    let browser = state.browser.lock().await;
    browser.settings.card_autofill
        && browser
            .tab(tab)
            .and_then(|tab| browser.workspace(tab.workspace_id))
            .is_ok_and(|workspace| !workspace.ephemeral)
}

/// `tab_id`'s submitted card, without its number, for the prompt.
#[tauri::command]
pub async fn card_capture_pending(
    state: State<'_, AppState>,
    tab_id: TabId,
) -> WisResult<Option<PendingCard>> {
    Ok(state
        .pending_cards
        .lock()
        .await
        .get(&tab_id)
        .map(pending_card))
}

/// Save the card `serial` names, the one the prompt showed for `tab_id`, or
/// renew the saved card with its number. Needs the vault unlocked, and the
/// tab still allowed to save.
#[tauri::command]
pub async fn card_capture_save(
    state: State<'_, AppState>,
    tab_id: TabId,
    serial: u64,
) -> WisResult<CardSaved> {
    if !may_save(&state, tab_id).await {
        return Err(WisError::InvalidInput {
            message: "Saving cards is turned off here".to_string(),
        });
    }
    let captured =
        waiting(&*state.pending_cards.lock().await, tab_id, serial).ok_or_else(|| {
            WisError::InvalidInput {
                message: "That card is no longer waiting to be saved".to_string(),
            }
        })?;
    let saved = crate::vault::cards::save_captured(&state, captured.card).await?;
    drop_shown(&mut *state.pending_cards.lock().await, tab_id, serial);
    Ok(saved)
}

/// Drop the card `serial` names. The tab's next prompt waits for a minute to
/// be opened.
#[tauri::command]
pub async fn card_capture_dismiss(
    state: State<'_, AppState>,
    tab_id: TabId,
    serial: u64,
) -> WisResult<()> {
    drop_shown(&mut *state.pending_cards.lock().await, tab_id, serial);
    hush(tab_id, Instant::now());
    Ok(())
}

/// The prompt for `tab_id` was closed without an answer: its card waits to
/// be opened, as does the next one for a minute.
#[tauri::command]
pub async fn card_capture_hush(state: State<'_, AppState>, tab_id: TabId) -> WisResult<()> {
    if let Some(held) = state.pending_cards.lock().await.get_mut(&tab_id) {
        held.quiet = true;
    }
    hush(tab_id, Instant::now());
    Ok(())
}

/// Drop a tab's submitted card.
pub async fn forget(state: &AppState, tab: TabId) {
    state.pending_cards.lock().await.remove(&tab);
    if let Some(last) = LAST_OFFER
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .as_mut()
    {
        last.remove(&tab);
    }
    if let Some(dismissed) = DISMISSED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .as_mut()
    {
        dismissed.remove(&tab);
    }
}

fn drop_shown(pending: &mut HashMap<TabId, Captured>, tab: TabId, serial: u64) {
    if pending.get(&tab).is_some_and(|held| held.serial == serial) {
        pending.remove(&tab);
    }
}

fn waiting(pending: &HashMap<TabId, Captured>, tab: TabId, serial: u64) -> Option<Captured> {
    pending
        .get(&tab)
        .filter(|captured| captured.serial == serial)
        .cloned()
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

/// Whether `tab` may offer a card now: once a second at most.
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

/// The card in a page's report, tidied, or None unless it holds a valid
/// number and expiry.
fn captured(values: Option<&serde_json::Value>) -> Option<Captured> {
    let values = values?.as_object()?;
    let number = Zeroizing::new(values.get("number")?.as_str()?.to_string());
    let small = |key: &str| {
        values
            .get(key)
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0)
    };
    let card = Card {
        number,
        name: values
            .get("name")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string(),
        exp_month: u8::try_from(small("month")).ok()?,
        exp_year: u16::try_from(small("year")).ok()?,
        nickname: String::new(),
    }
    .tidied()
    .ok()?;
    Some(Captured {
        card,
        serial: SERIAL.fetch_add(1, Ordering::Relaxed),
        quiet: false,
    })
}

/// Whether a card is already saved: the same network, last four digits
/// and expiry. Read from what saved cards show without the vault key.
fn already_listed(saved: &[CardSummary], card: &Card) -> bool {
    let wanted = card.summary();
    saved.iter().any(|listed| {
        listed.brand == wanted.brand
            && listed.last4 == wanted.last4
            && listed.exp_month == wanted.exp_month
            && listed.exp_year == wanted.exp_year
    })
}

fn pending_card(captured: &Captured) -> PendingCard {
    PendingCard {
        serial: captured.serial,
        summary: captured.card.summary(),
        name: captured.card.name.clone(),
        quiet: captured.quiet,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values(number: &str, month: u64, year: u64) -> serde_json::Value {
        serde_json::json!({ "number": number, "name": " Ada Lovelace ", "month": month, "year": year })
    }

    #[test]
    fn a_submitted_card_is_tidied_and_given_a_serial() {
        let one = captured(Some(&values("4242 4242 4242 4242", 8, 28))).unwrap();
        let two = captured(Some(&values("4242424242424242", 8, 2028))).unwrap();
        assert_eq!(*one.card.number, "4242424242424242");
        assert_eq!(one.card.name, "Ada Lovelace");
        assert_eq!((one.card.exp_month, one.card.exp_year), (8, 2028));
        assert_eq!(one.card.nickname, "");
        assert_ne!(one.serial, two.serial);
    }

    #[test]
    fn a_report_without_a_valid_number_or_expiry_is_dropped() {
        assert!(captured(Some(&values("4242 4242 4242 4241", 8, 28))).is_none());
        assert!(captured(Some(&values("4242424242424242", 0, 28))).is_none());
        assert!(captured(Some(&values("4242424242424242", 8, 0))).is_none());
        assert!(captured(Some(&serde_json::json!({ "number": 4242 }))).is_none());
        assert!(captured(None).is_none());
    }

    #[test]
    fn what_the_prompt_shows_never_holds_the_number() {
        let shown = pending_card(&captured(Some(&values("4242424242424242", 8, 28))).unwrap());
        let json = serde_json::to_string(&shown).unwrap();
        assert!(!json.contains("4242424242424242"));
        assert!(json.contains("\"last4\":\"4242\""));
        assert!(json.contains("\"name\":\"Ada Lovelace\""));
    }

    #[test]
    fn a_card_listed_the_same_is_not_offered_again() {
        let card = captured(Some(&values("4242424242424242", 8, 28)))
            .unwrap()
            .card;
        let listed = card.summary();
        assert!(already_listed(std::slice::from_ref(&listed), &card));
        let renamed = CardSummary {
            nickname: "Travel".into(),
            ..listed.clone()
        };
        assert!(
            already_listed(&[renamed], &card),
            "a nickname is the user's own"
        );
        let renewed = CardSummary {
            exp_year: 2031,
            ..listed
        };
        assert!(!already_listed(&[renewed], &card));
    }

    #[test]
    fn a_dismissed_tab_waits_a_minute_to_be_offered_again() {
        let tab = TabId::from_uuid(uuid::Uuid::new_v4());
        let at = Instant::now();
        assert!(!hushed(tab, at));
        hush(tab, at);
        assert!(hushed(tab, at + Duration::from_secs(30)));
        assert!(!hushed(tab, at + Duration::from_secs(61)));
    }

    #[test]
    fn a_tab_may_offer_a_card_once_a_second() {
        let tab = TabId::from_uuid(uuid::Uuid::new_v4());
        let at = Instant::now();
        assert!(offer_allowed(tab, at));
        assert!(!offer_allowed(tab, at + Duration::from_millis(500)));
        assert!(offer_allowed(tab, at + Duration::from_millis(1100)));
    }
}

//! Saved payment cards. The summary (nickname, network, last four digits,
//! expiry) lists without the vault key. The card, number and name included,
//! is sealed whole, so saving, editing and deleting need the vault unlocked.
//! The full number never goes to the chrome.

use serde::Serialize;
use tauri::State;
use wis_core::vault::card::{Card, CardBrand, CardSummary, MAX_CARDS};
use wis_core::{WisError, WisResult};
use wis_secrets::{EncryptedBlob, MasterKey};
use wis_storage::{CardRecord, CardRow};
use zeroize::Zeroizing;

use crate::state::AppState;
use crate::vault::passwords::{locked, vault_err};
use crate::vault::store::blocking;

const PURPOSE: &[u8] = b"wis/card/v1";

#[derive(Serialize)]
pub struct SavedCard {
    pub id: i64,
    #[serde(flatten)]
    pub summary: CardSummary,
}

/// A saved card for its edit form: everything but the number.
#[derive(Serialize)]
pub struct CardDetails {
    pub name: String,
    pub exp_month: u8,
    pub exp_year: u16,
    pub nickname: String,
}

/// Every saved card's summary; needs no unlock.
#[tauri::command]
pub async fn card_list(state: State<'_, AppState>) -> WisResult<Vec<SavedCard>> {
    let storage = state.storage.clone();
    let rows = blocking(move || storage.list_cards().map_err(persist_err)).await?;
    Ok(rows.iter().map(summary).collect())
}

/// Saved card `id` without its number, for editing.
#[tauri::command]
pub async fn card_details(state: State<'_, AppState>, id: i64) -> WisResult<CardDetails> {
    let guard = state.vault_key.lock().await;
    let key = guard.as_ref().ok_or_else(locked)?;
    let storage = state.storage.clone();
    let rows = blocking(move || storage.list_cards().map_err(persist_err)).await?;
    let row = rows
        .into_iter()
        .find(|row| row.id == id)
        .ok_or_else(|| invalid("This card was removed"))?;
    let card = open(key, row.ciphertext, row.nonce)?;
    drop(guard);
    Ok(CardDetails {
        name: card.name.clone(),
        exp_month: card.exp_month,
        exp_year: card.exp_year,
        nickname: card.nickname.clone(),
    })
}

/// Save `card` as a new one, or in place of the one with `id`, where an empty
/// number keeps the saved one. Returns its id.
#[tauri::command]
pub async fn card_save(state: State<'_, AppState>, id: Option<i64>, card: Card) -> WisResult<i64> {
    let guard = state.vault_key.lock().await;
    let key = guard.as_ref().ok_or_else(locked)?;
    let storage = state.storage.clone();
    let rows = blocking(move || storage.list_cards().map_err(persist_err)).await?;
    let stored = rows.len();
    let known: Vec<(i64, Card)> = readable(key, rows).collect();
    let card = to_save(&known, stored, id, card)?;
    let sealed = seal(key, &card)?;
    let summary = card.summary();
    let storage = state.storage.clone();
    let saved = blocking(move || {
        storage
            .save_card(id, &row(&summary, &sealed))
            .map_err(persist_err)
    })
    .await;
    drop(guard);
    saved
}

/// Every saved card, for an already authorized backup.
pub(crate) async fn export_cards(state: &State<'_, AppState>) -> WisResult<Vec<Card>> {
    let guard = state.vault_key.lock().await;
    let key = guard.as_ref().ok_or_else(locked)?;
    let storage = state.storage.clone();
    let rows = blocking(move || storage.list_cards().map_err(persist_err)).await?;
    Ok(readable(key, rows).map(|(_, card)| card).collect())
}

/// Seal and store restored cards not already saved, up to the limit, for an
/// import already authorized. Returns how many were added.
pub(crate) async fn import_cards(
    state: &State<'_, AppState>,
    incoming: Vec<Card>,
) -> WisResult<usize> {
    if incoming.is_empty() {
        return Ok(0);
    }

    let guard = state.vault_key.lock().await;
    let key = guard.as_ref().ok_or_else(locked)?;
    let storage = state.storage.clone();
    let rows = blocking(move || storage.list_cards().map_err(persist_err)).await?;
    let stored = rows.len();
    let known: Vec<Card> = readable(key, rows).map(|(_, card)| card).collect();
    let sealed = to_add(&known, stored, incoming)?
        .iter()
        .map(|card| Ok((card.summary(), seal(key, card)?)))
        .collect::<WisResult<Vec<_>>>()?;
    let added = sealed.len();
    let storage = state.storage.clone();
    let saved = blocking(move || {
        sealed.iter().try_for_each(|(summary, blob)| {
            storage
                .save_card(None, &row(summary, blob))
                .map(|_| ())
                .map_err(persist_err)
        })
    })
    .await;
    drop(guard);
    saved.map(|()| added)
}

fn row<'a>(summary: &'a CardSummary, sealed: &'a EncryptedBlob) -> CardRow<'a> {
    CardRow {
        nickname: &summary.nickname,
        brand: summary.brand.as_str(),
        last4: &summary.last4,
        exp_month: u32::from(summary.exp_month),
        exp_year: u32::from(summary.exp_year),
        ciphertext: &sealed.ciphertext,
        nonce: &sealed.nonce,
    }
}

#[tauri::command]
pub async fn card_delete(state: State<'_, AppState>, id: i64) -> WisResult<()> {
    let guard = state.vault_key.lock().await;
    guard.as_ref().ok_or_else(locked)?;
    let storage = state.storage.clone();
    let deleted = blocking(move || storage.delete_card(id).map_err(persist_err)).await;
    drop(guard);
    deleted
}

/// Whether `row` opens with `key`.
pub(crate) fn opens(key: &MasterKey, row: &CardRecord) -> bool {
    open(key, row.ciphertext.clone(), row.nonce.clone()).is_ok()
}

/// The card to store for a save of `incoming` in place of `id` (or new):
/// the saved number kept when none is given, tidied, not a second copy of a
/// saved card and within the limit given `stored` rows.
fn to_save(
    known: &[(i64, Card)],
    stored: usize,
    id: Option<i64>,
    incoming: Card,
) -> WisResult<Card> {
    let card = match id {
        Some(id) => {
            let (_, saved) = known
                .iter()
                .find(|(known_id, _)| *known_id == id)
                .ok_or_else(|| invalid("This card was removed"))?;
            if incoming.number.trim().is_empty() {
                Card {
                    number: saved.number.clone(),
                    ..incoming
                }
            } else {
                incoming
            }
        }
        None if stored >= MAX_CARDS => {
            return Err(invalid(&format!("Wis keeps up to {MAX_CARDS} cards")));
        }
        None => incoming,
    }
    .tidied()?;
    if known
        .iter()
        .any(|(known_id, saved)| Some(*known_id) != id && saved.number == card.number)
    {
        return Err(invalid("This card is already saved"));
    }
    Ok(card)
}

/// The incoming cards worth keeping: tidied, not already saved or given
/// twice, and within the limit given `stored` rows.
fn to_add(known: &[Card], stored: usize, incoming: Vec<Card>) -> WisResult<Vec<Card>> {
    let room = MAX_CARDS.saturating_sub(stored);
    let mut kept: Vec<Card> = Vec::new();
    for card in incoming {
        let card = card.tidied()?;
        let saved = |other: &Card| other.number == card.number;
        if kept.len() < room && !known.iter().any(saved) && !kept.iter().any(saved) {
            kept.push(card);
        }
    }
    Ok(kept)
}

fn summary(row: &CardRecord) -> SavedCard {
    SavedCard {
        id: row.id,
        summary: CardSummary {
            nickname: row.nickname.clone(),
            brand: CardBrand::parse(&row.brand),
            last4: row.last4.clone(),
            exp_month: u8::try_from(row.exp_month).unwrap_or(0),
            exp_year: u16::try_from(row.exp_year).unwrap_or(0),
        },
    }
}

fn readable(key: &MasterKey, rows: Vec<CardRecord>) -> impl Iterator<Item = (i64, Card)> + '_ {
    rows.into_iter()
        .filter_map(move |row| match open(key, row.ciphertext, row.nonce) {
            Ok(card) => Some((row.id, card)),
            Err(err) => {
                log::warn!("vault: saved card {} could not be opened: {err}", row.id);
                None
            }
        })
}

fn open(key: &MasterKey, ciphertext: Vec<u8>, nonce: Vec<u8>) -> WisResult<Card> {
    let plaintext = Zeroizing::new(
        wis_secrets::decrypt_for(key, &EncryptedBlob { ciphertext, nonce }, PURPOSE)
            .map_err(vault_err)?,
    );
    serde_json::from_slice(&plaintext).map_err(|_| invalid("A saved card is damaged"))
}

fn seal(key: &MasterKey, card: &Card) -> WisResult<EncryptedBlob> {
    let json = Zeroizing::new(serde_json::to_vec(card).map_err(persist_err)?);
    wis_secrets::encrypt_for(key, &json, PURPOSE).map_err(vault_err)
}

fn persist_err(err: impl std::fmt::Display) -> WisError {
    WisError::PersistenceFailed {
        reason: err.to_string(),
    }
}

fn invalid(message: &str) -> WisError {
    WisError::InvalidInput {
        message: message.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn visa() -> Card {
        Card {
            number: Zeroizing::new("4242424242424242".into()),
            name: "Ada Lovelace".into(),
            exp_month: 8,
            exp_year: 2028,
            nickname: "Travel".into(),
        }
    }

    fn mastercard() -> Card {
        Card {
            number: Zeroizing::new("5555555555554444".into()),
            ..visa()
        }
    }

    #[test]
    fn a_card_opens_only_with_its_key_and_only_as_a_card() {
        let key = wis_secrets::generate_master_key();
        let blob = seal(&key, &visa()).unwrap();
        let opened = open(&key, blob.ciphertext.clone(), blob.nonce.clone()).unwrap();
        assert_eq!(opened, visa());
        let other = wis_secrets::generate_master_key();
        assert!(open(&other, blob.ciphertext.clone(), blob.nonce.clone()).is_err());
        assert!(wis_secrets::decrypt(&key, &blob).is_err());
    }

    #[test]
    fn an_edit_without_a_number_keeps_the_saved_one() {
        let edit = Card {
            number: Zeroizing::default(),
            nickname: "Work".into(),
            ..visa()
        };
        let saved = to_save(&[(7, visa())], 1, Some(7), edit).unwrap();
        assert_eq!(saved.number, visa().number);
        assert_eq!(saved.nickname, "Work");
    }

    #[test]
    fn a_new_card_needs_a_number() {
        let blank = Card {
            number: Zeroizing::default(),
            ..visa()
        };
        assert!(to_save(&[], 0, None, blank).is_err());
    }

    #[test]
    fn a_card_already_saved_is_not_saved_twice() {
        let spaced = Card {
            number: Zeroizing::new("4242 4242 4242 4242".into()),
            ..visa()
        };
        let err = to_save(&[(7, visa())], 1, None, spaced.clone()).unwrap_err();
        assert_eq!(
            err.to_string(),
            invalid("This card is already saved").to_string()
        );
        assert!(to_save(&[(7, visa())], 1, Some(7), spaced).is_ok());
        assert!(to_save(&[(7, visa()), (8, mastercard())], 2, Some(8), visa()).is_err());
    }

    #[test]
    fn restored_cards_skip_ones_already_saved_given_twice_or_beyond_the_limit() {
        let spaced = Card {
            number: Zeroizing::new("4242 4242 4242 4242".into()),
            ..visa()
        };
        let kept = to_add(&[visa()], 1, vec![spaced, mastercard(), mastercard()]).unwrap();
        assert_eq!(kept, vec![mastercard()]);
        assert!(to_add(&[], MAX_CARDS, vec![visa()]).unwrap().is_empty());
        assert!(to_add(&[], 0, vec![Card::default()]).is_err());
    }

    #[test]
    fn a_removed_card_or_one_beyond_the_limit_is_refused() {
        assert!(to_save(&[], 0, Some(7), visa()).is_err());
        assert!(to_save(&[], MAX_CARDS, None, visa()).is_err());
    }
}

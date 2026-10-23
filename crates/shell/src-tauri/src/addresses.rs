//! Saved addresses, each sealed whole with the vault key, so listing, saving
//! and deleting need the vault unlocked.

use serde::Serialize;
use tauri::State;
use wis_core::vault::address::{Address, MAX_ADDRESSES};
use wis_core::{WisError, WisResult};
use wis_secrets::{EncryptedBlob, MasterKey};
use wis_storage::AddressRecord;
use zeroize::Zeroizing;

use crate::state::AppState;
use crate::vault::{locked, vault_err};
use crate::vault_store::blocking;

const PURPOSE: &[u8] = b"wis/address/v1";
const SEALED_JSON_CAPACITY: usize = 16 * 1024;

#[derive(Serialize)]
pub struct SavedAddress {
    pub id: i64,
    pub address: Address,
}

#[tauri::command]
pub async fn address_list(state: State<'_, AppState>) -> WisResult<Vec<SavedAddress>> {
    let guard = state.vault_key.lock().await;
    let key = guard.as_ref().ok_or_else(locked)?;
    let storage = state.storage.clone();
    let rows = blocking(move || storage.list_addresses().map_err(persist_err)).await?;
    Ok(readable(key, rows)
        .map(|(id, address)| SavedAddress { id, address })
        .collect())
}

/// Save `address` as a new one, or in place of the one with `id`. Returns its id.
#[tauri::command]
pub async fn address_save(
    state: State<'_, AppState>,
    id: Option<i64>,
    address: Address,
) -> WisResult<i64> {
    let address = address.tidied()?;
    let guard = state.vault_key.lock().await;
    let key = guard.as_ref().ok_or_else(locked)?;
    let blob = seal(key, &address)?;
    let storage = state.storage.clone();
    let saved = blocking(move || {
        let rows = storage.list_addresses().map_err(persist_err)?;
        match id {
            Some(id) if !rows.iter().any(|row| row.id == id) => {
                return Err(invalid("This address was removed"));
            }
            None if rows.len() >= MAX_ADDRESSES => {
                return Err(invalid(&format!(
                    "Wis keeps up to {MAX_ADDRESSES} addresses"
                )));
            }
            _ => {}
        }
        storage
            .save_address(id, &blob.ciphertext, &blob.nonce)
            .map_err(persist_err)
    })
    .await;
    drop(guard);
    saved
}

/// Whether any address is saved, requiring no unlock.
#[tauri::command]
pub async fn address_any(state: State<'_, AppState>) -> WisResult<bool> {
    let storage = state.storage.clone();
    blocking(move || storage.has_addresses().map_err(persist_err)).await
}

#[tauri::command]
pub async fn address_delete(state: State<'_, AppState>, id: i64) -> WisResult<()> {
    let guard = state.vault_key.lock().await;
    guard.as_ref().ok_or_else(locked)?;
    let storage = state.storage.clone();
    let deleted = blocking(move || storage.delete_address(id).map_err(persist_err)).await;
    drop(guard);
    deleted
}

/// Every saved address that opens, in the clear, for a backup already
/// authorized.
pub(crate) async fn export_addresses(state: &State<'_, AppState>) -> WisResult<Vec<Address>> {
    let guard = state.vault_key.lock().await;
    let key = guard.as_ref().ok_or_else(locked)?;
    let storage = state.storage.clone();
    let rows = blocking(move || storage.list_addresses().map_err(persist_err)).await?;
    Ok(readable(key, rows).map(|(_, address)| address).collect())
}

/// Seal and store restored addresses not already saved, up to the limit, for
/// an import already authorized. Returns how many were added.
pub(crate) async fn import_addresses(
    state: &State<'_, AppState>,
    incoming: Vec<Address>,
) -> WisResult<usize> {
    if incoming.is_empty() {
        return Ok(0);
    }

    let guard = state.vault_key.lock().await;
    let key = guard.as_ref().ok_or_else(locked)?;
    let storage = state.storage.clone();
    let rows = blocking(move || storage.list_addresses().map_err(persist_err)).await?;
    let stored = rows.len();
    let known: Vec<Address> = readable(key, rows).map(|(_, address)| address).collect();
    let sealed = to_add(&known, stored, incoming)?
        .iter()
        .map(|address| seal(key, address))
        .collect::<WisResult<Vec<_>>>()?;
    let added = sealed.len();
    let storage = state.storage.clone();
    let saved = blocking(move || {
        sealed.iter().try_for_each(|blob| {
            storage
                .save_address(None, &blob.ciphertext, &blob.nonce)
                .map(|_| ())
                .map_err(persist_err)
        })
    })
    .await;
    drop(guard);
    saved.map(|()| added)
}

/// Seal and store `address` as a new one unless `covered` finds it among the
/// saved ones. Returns whether it was stored.
pub(crate) async fn add_unless(
    state: &AppState,
    address: Address,
    covered: impl FnOnce(&[Address]) -> bool,
) -> WisResult<bool> {
    let address = address.tidied()?;
    let guard = state.vault_key.lock().await;
    let key = guard.as_ref().ok_or_else(locked)?;
    let storage = state.storage.clone();
    let rows = blocking(move || storage.list_addresses().map_err(persist_err)).await?;
    let stored = rows.len();
    let known: Vec<Address> = readable(key, rows).map(|(_, address)| address).collect();
    if covered(&known) {
        return Ok(false);
    }
    if stored >= MAX_ADDRESSES {
        return Err(invalid(&format!(
            "Wis keeps up to {MAX_ADDRESSES} addresses"
        )));
    }

    let blob = seal(key, &address)?;
    let storage = state.storage.clone();
    let saved = blocking(move || {
        storage
            .save_address(None, &blob.ciphertext, &blob.nonce)
            .map(|_| ())
            .map_err(persist_err)
    })
    .await;
    drop(guard);
    saved.map(|()| true)
}

/// Saved address `id` in the clear, for a fill the user chose.
pub(crate) async fn opened(state: &State<'_, AppState>, id: i64) -> WisResult<Address> {
    let guard = state.vault_key.lock().await;
    let key = guard.as_ref().ok_or_else(locked)?;
    let storage = state.storage.clone();
    let rows = blocking(move || storage.list_addresses().map_err(persist_err)).await?;
    let row = rows
        .into_iter()
        .find(|row| row.id == id)
        .ok_or_else(|| invalid("This address was removed"))?;
    open(key, row.ciphertext, row.nonce)
}

/// Whether `row` opens with `key`.
pub(crate) fn opens(key: &MasterKey, row: &AddressRecord) -> bool {
    open(key, row.ciphertext.clone(), row.nonce.clone()).is_ok()
}

/// The incoming addresses worth keeping: tidied, not already saved or given
/// twice, and within the limit given `stored` rows.
fn to_add(known: &[Address], stored: usize, incoming: Vec<Address>) -> WisResult<Vec<Address>> {
    let room = MAX_ADDRESSES.saturating_sub(stored);
    let mut kept: Vec<Address> = Vec::new();
    for address in incoming {
        let address = address.tidied()?;
        if kept.len() < room && !known.contains(&address) && !kept.contains(&address) {
            kept.push(address);
        }
    }
    Ok(kept)
}

fn readable(
    key: &MasterKey,
    rows: Vec<AddressRecord>,
) -> impl Iterator<Item = (i64, Address)> + '_ {
    rows.into_iter()
        .filter_map(move |row| match open(key, row.ciphertext, row.nonce) {
            Ok(address) => Some((row.id, address)),
            Err(err) => {
                log::warn!("vault: saved address {} could not be opened: {err}", row.id);
                None
            }
        })
}

fn open(key: &MasterKey, ciphertext: Vec<u8>, nonce: Vec<u8>) -> WisResult<Address> {
    let plaintext = Zeroizing::new(
        wis_secrets::decrypt_for(key, &EncryptedBlob { ciphertext, nonce }, PURPOSE)
            .map_err(vault_err)?,
    );
    serde_json::from_slice(&plaintext).map_err(|_| invalid("A saved address is damaged"))
}

fn seal(key: &MasterKey, address: &Address) -> WisResult<EncryptedBlob> {
    let mut json = Zeroizing::new(Vec::with_capacity(SEALED_JSON_CAPACITY));
    serde_json::to_writer(&mut *json, address).map_err(persist_err)?;
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

    fn home() -> Address {
        Address {
            given_name: "Ada".into(),
            city: "London".into(),
            country: "GB".into(),
            ..Address::default()
        }
    }

    #[test]
    fn an_address_opens_only_with_its_key_and_only_as_an_address() {
        let key = wis_secrets::generate_master_key();
        let blob = seal(&key, &home()).unwrap();

        let opened = open(&key, blob.ciphertext.clone(), blob.nonce.clone()).unwrap();
        assert_eq!(opened, home());
        let other = wis_secrets::generate_master_key();
        assert!(open(&other, blob.ciphertext.clone(), blob.nonce.clone()).is_err());
        assert!(wis_secrets::decrypt(&key, &blob).is_err());
    }

    #[test]
    fn a_damaged_address_says_so_without_its_contents() {
        let key = wis_secrets::generate_master_key();
        let blob = wis_secrets::encrypt_for(&key, b"{\"city\": 12", PURPOSE).unwrap();
        let err = open(&key, blob.ciphertext, blob.nonce).unwrap_err();
        assert_eq!(
            err.to_string(),
            invalid("A saved address is damaged").to_string()
        );
    }

    #[test]
    fn restored_addresses_skip_ones_already_saved_given_twice_or_beyond_the_limit() {
        let spaced = Address {
            country: " gb ".into(),
            ..home()
        };
        let work = Address {
            city: "Paris".into(),
            ..home()
        };
        let kept = to_add(&[home()], 1, vec![spaced, work.clone(), work.clone()]).unwrap();
        assert_eq!(kept, vec![work.clone()]);

        assert!(to_add(&[], MAX_ADDRESSES, vec![work.clone()])
            .unwrap()
            .is_empty());
        assert!(to_add(&[], 0, vec![Address::default()]).is_err());
    }
}

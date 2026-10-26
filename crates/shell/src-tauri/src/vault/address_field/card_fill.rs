//! Filling a saved payment card at the card field the page reported. Every
//! fill asks Touch ID or the vault password, and only https pages are filled.

use serde::Serialize;
use tauri::{AppHandle, Manager, State};
use wis_core::vault::card::{Card, CardBrand};
use wis_core::{TabId, WisError, WisResult};
use zeroize::Zeroizing;

use crate::state::AppState;

use super::card_scripts::{CARD_FILL_JS, CARD_JS};
use super::scripts::CLASSIFY_JS;
use super::{invalid, origin_of, take, Form, Offered};

/// Fill saved card `id` into the card field report `token` names, after
/// Touch ID or the vault password, if that report is still the tab's last and
/// the page is still on its https origin. Runs in Wis's own script world.
#[tauri::command]
pub async fn card_fill(
    app: AppHandle,
    state: State<'_, AppState>,
    tab_id: TabId,
    id: i64,
    token: String,
    vault_password: Option<String>,
) -> WisResult<()> {
    let vault_password = vault_password.map(Zeroizing::new);
    if !state.browser.lock().await.settings.card_autofill {
        return Err(invalid("Filling cards is turned off"));
    }

    let tab = tab_id.as_uuid().to_string();
    let label = format!("tab-{tab}");
    let expected = super::current(&tab, &token, Form::Card).ok_or_else(moved_on)?;
    if !card_origin(&expected) || origin_of(&app, &label).as_deref() != Some(expected.as_str()) {
        return Err(moved_on());
    }

    crate::vault::passwords::authorize_vault(
        &state,
        &fill_reason(&expected),
        vault_password.as_deref().map(String::as_str),
    )
    .await?;

    let card = crate::vault::cards::opened(&state, id).await?;
    let offered = take(&tab, &token, Form::Card).ok_or_else(moved_on)?;
    let webview = app
        .webviews()
        .get(&label)
        .cloned()
        .ok_or_else(|| invalid("That tab is gone"))?;
    let origin = origin_of(&app, &label);
    if origin.as_deref() != Some(offered.origin.as_str()) || !card_origin(&offered.origin) {
        return Err(moved_on());
    }

    let script = card_fill_script(&offered, &card)?;
    crate::platform::content_world::eval_secret(&webview, script).map_err(|err| {
        WisError::InvalidInput {
            message: format!("card fill failed: {err}"),
        }
    })
}

/// The Touch ID reason, naming the site the card would go to.
fn fill_reason(origin: &str) -> String {
    let host = origin.strip_prefix("https://").unwrap_or(origin);
    format!("Fill your saved card on {host}")
}

/// Whether a card may be offered or filled on `origin`.
pub(super) fn card_origin(origin: &str) -> bool {
    origin.starts_with("https://")
}

fn moved_on() -> WisError {
    invalid("The page has moved on")
}

/// The names a card type list may give `brand`, lowercase.
fn brand_names(brand: CardBrand) -> &'static [&'static str] {
    match brand {
        CardBrand::Visa => &["visa"],
        CardBrand::Mastercard => &["mastercard", "master card", "mc"],
        CardBrand::Amex => &["amex", "american express", "americanexpress"],
        CardBrand::Discover => &["discover"],
        CardBrand::Jcb => &["jcb"],
        CardBrand::Diners => &["diners", "diners club", "dinersclub"],
        CardBrand::Unionpay => &["unionpay", "union pay", "cup"],
        CardBrand::Other => &[],
    }
}

/// Room for the fill's values.
const VALUES_CAPACITY: usize = 4096;

#[derive(Serialize)]
struct Values<'a> {
    number: &'a str,
    name: &'a str,
    month: u8,
    year: u16,
    brand: &'static [&'static str],
}

/// The fill for `offered`: its origin and token, and only the values its
/// kinds reach. Never the security code, which is not kept. Built in buffers
/// sized up front, so no copy of the number is left behind by a regrowth.
fn card_fill_script(offered: &Offered, card: &Card) -> WisResult<Zeroizing<String>> {
    let reaches = |kinds: &[&str]| kinds.iter().any(|kind| offered.kinds.contains(kind));
    let expiry = reaches(&["exp", "month", "year"]);
    let values = Values {
        number: if reaches(&["number"]) {
            &card.number
        } else {
            ""
        },
        name: if reaches(&["name", "given", "family"]) {
            &card.name
        } else {
            ""
        },
        month: if expiry { card.exp_month } else { 0 },
        year: if expiry { card.exp_year } else { 0 },
        brand: if reaches(&["type"]) {
            brand_names(CardBrand::of(&card.number))
        } else {
            &[]
        },
    };
    let mut json = Zeroizing::new(Vec::with_capacity(VALUES_CAPACITY));
    serde_json::to_writer(&mut *json, &values).map_err(|err| invalid(&err.to_string()))?;
    let json = std::str::from_utf8(&json).map_err(|err| invalid(&err.to_string()))?;
    let origin = serde_json::Value::String(offered.origin.clone()).to_string();
    let token = serde_json::Value::String(offered.token.clone()).to_string();

    let parts = [
        "(function () {\nvar ORIGIN = ",
        &origin,
        ";\nvar TOKEN = ",
        &token,
        ";\nvar V = ",
        json,
        ";\n",
        CLASSIFY_JS,
        "\n",
        CARD_JS,
        "\n",
        CARD_FILL_JS,
        "\n})();",
    ];
    let mut script = Zeroizing::new(String::with_capacity(
        parts.iter().map(|part| part.len()).sum(),
    ));
    parts.iter().for_each(|part| script.push_str(part));
    Ok(script)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOKEN: &str = "0123456789abcdef0123456789abcdef";

    fn visa() -> Card {
        Card {
            number: Zeroizing::new("4242424242424242".into()),
            name: "Ada \"Lovelace\"".into(),
            exp_month: 8,
            exp_year: 2028,
            nickname: "Travel".into(),
        }
    }

    fn offer(kinds: Vec<&'static str>) -> Offered {
        Offered {
            origin: "https://shop.example".into(),
            form: Form::Card,
            kinds,
            token: TOKEN.into(),
        }
    }

    #[test]
    fn a_card_fill_carries_only_the_values_its_fields_reach() {
        let script = card_fill_script(&offer(vec!["number", "exp"]), &visa()).unwrap();
        assert!(script.contains("var ORIGIN = \"https://shop.example\";"));
        assert!(script.contains(&format!("var TOKEN = \"{TOKEN}\";")));
        assert!(script.contains("\"number\":\"4242424242424242\""));
        assert!(script.contains("\"month\":8"));
        assert!(script.contains("\"year\":2028"));
        for kept_out in ["Lovelace", "Travel", "\"visa\""] {
            assert!(!script.contains(kept_out), "{kept_out}");
        }
        assert!(script.contains("function cardSection("));
        assert!(
            script.find("var CARD_KINDS").unwrap()
                < script
                    .find("var anchor = window.__wisAddressAnchor")
                    .unwrap()
        );
    }

    #[test]
    fn a_name_is_quoted_and_a_type_list_gets_the_network_names() {
        let script = card_fill_script(&offer(vec!["number", "name", "type"]), &visa()).unwrap();
        assert!(script.contains(r#""name":"Ada \"Lovelace\"""#));
        assert!(script.contains(r#""brand":["visa"]"#));
    }

    #[test]
    fn the_script_fills_one_buffer_of_exactly_its_size() {
        let script = card_fill_script(&offer(vec!["number", "name"]), &visa()).unwrap();
        assert_eq!(script.capacity(), script.len());
        assert!(script.starts_with("(function () {\nvar ORIGIN = \"https://shop.example\";"));
        assert!(script.ends_with("\n})();"));
    }

    #[test]
    fn the_touch_id_reason_names_the_site() {
        assert_eq!(
            fill_reason("https://shop.example"),
            "Fill your saved card on shop.example"
        );
    }

    #[test]
    fn cards_are_only_for_https_pages() {
        assert!(card_origin("https://shop.example"));
        assert!(!card_origin("http://shop.example"));
        assert!(!card_origin("file://"));
        assert!(!card_origin(""));
    }

    #[test]
    fn a_card_fill_takes_only_a_card_offer() {
        let tab = "tab-test-card-take";
        super::super::remember(
            tab,
            Offered {
                form: Form::Address,
                ..offer(vec!["postal"])
            },
        );
        assert!(take(tab, TOKEN, Form::Card).is_none());
        super::super::remember(tab, offer(vec!["number"]));
        assert!(take(tab, TOKEN, Form::Address).is_none());
        super::super::remember(tab, offer(vec!["number"]));
        assert!(take(tab, TOKEN, Form::Card).is_some());
    }
}

//! Tying an extension installed from a folder or `.zip` to its Chrome Web
//! Store listing, so it updates like one installed from the store.

use serde::{Deserialize, Serialize};

use crate::extensions::{is_store_id, tidy_name, with_changed, InstalledExtension};
use crate::{WisError, WisResult};

/// Listings said not to be an extension.
pub const MAX_DECLINED: usize = 32;
/// How long a search that found nothing waits before looking again.
pub const SEARCH_AGAIN_AFTER: u64 = 7 * 24 * 60 * 60;

/// A listing the daily search found for an extension not tied to one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoreSuggestion {
    pub store_id: String,
    /// Its package at the installed version has the same files.
    pub same_files: bool,
}

/// The longest slug the store puts in a listing's address.
const MAX_SLUG: usize = 25;

/// A listing a store search page links to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listing {
    pub store_id: String,
    /// The listing's name taken from its address.
    pub slug: Option<String>,
}

/// Whether two extension names are the same name, as Wis shows them. A name
/// with nothing in it matches nothing.
pub fn same_name(a: &str, b: &str) -> bool {
    !a.trim().is_empty()
        && !b.trim().is_empty()
        && tidy_name(a).to_lowercase() == tidy_name(b).to_lowercase()
}

/// How the store spells a listing called `name` in its address: lowercase
/// letters and digits, dashes between, cut to 25 characters. `None` for a
/// name with no such letters.
pub fn listing_slug(name: &str) -> Option<String> {
    let words: Vec<String> = name
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_ascii_lowercase)
        .collect();
    if words.is_empty() {
        return None;
    }
    let cut: String = words.join("-").chars().take(MAX_SLUG).collect();
    Some(cut.trim_end_matches('-').to_string())
}

/// The listings a store search page links to, once each, in its order.
pub fn listings(html: &str) -> Vec<Listing> {
    let mut found: Vec<Listing> = Vec::new();
    for (at, _) in html.match_indices("detail/") {
        let path = &html[at + "detail/".len()..];
        let end = path
            .find(|c: char| c.is_whitespace() || "\"'?#<>".contains(c))
            .unwrap_or(path.len());
        let mut segments = path[..end].split('/');
        let listing = match (segments.next(), segments.next()) {
            (Some(id), _) if is_store_id(id) => Listing {
                store_id: id.to_string(),
                slug: None,
            },
            (Some(slug), Some(id)) if is_store_id(id) && !slug.is_empty() => Listing {
                store_id: id.to_string(),
                slug: Some(slug.to_string()),
            },
            _ => continue,
        };
        if !found.iter().any(|known| known.store_id == listing.store_id) {
            found.push(listing);
        }
    }

    found
}

/// Ties `id` to the listing `store_id`, for its updates.
pub fn with_store_link(
    list: &[InstalledExtension],
    id: &str,
    store_id: &str,
) -> WisResult<Vec<InstalledExtension>> {
    let refuse = |message: &str| {
        Err(WisError::InvalidInput {
            message: message.into(),
        })
    };
    if !is_store_id(store_id) {
        return refuse("not a Chrome Web Store id");
    }
    if list
        .iter()
        .any(|e| e.id != id && e.store_id.as_deref() == Some(store_id))
    {
        return refuse("that listing belongs to another installed extension");
    }
    if list.iter().any(|e| e.id == id && e.store_id.is_some()) {
        return refuse("that extension already gets its updates from the store");
    }

    with_changed(list, id, |ext| {
        ext.store_id = Some(store_id.to_string());
        ext.store_suggestion = None;
    })
}

/// Keeps what the daily search found, unless otherwise specified by
/// the user.
pub fn with_store_suggestion(
    list: &[InstalledExtension],
    id: &str,
    suggestion: Option<StoreSuggestion>,
) -> WisResult<Vec<InstalledExtension>> {
    with_changed(list, id, |ext| {
        ext.store_suggestion = suggestion.filter(|s| !ext.declined_listings.contains(&s.store_id));
    })
}

/// Whether the daily search should look for `ext`'s listing at `now`.
pub fn wants_search(ext: &InstalledExtension, now: u64) -> bool {
    ext.store_id.is_none()
        && ext.store_suggestion.is_none()
        && ext
            .store_searched_at
            .is_none_or(|at| at > now || now - at >= SEARCH_AGAIN_AFTER)
}

pub fn with_store_searched(
    list: &[InstalledExtension],
    id: &str,
    at: u64,
) -> WisResult<Vec<InstalledExtension>> {
    with_changed(list, id, |ext| ext.store_searched_at = Some(at))
}

/// Remembers that the listing `store_id` is not `id`'s.
pub fn with_declined_listing(
    list: &[InstalledExtension],
    id: &str,
    store_id: &str,
) -> WisResult<Vec<InstalledExtension>> {
    if !is_store_id(store_id) {
        return Err(WisError::InvalidInput {
            message: "not a Chrome Web Store id".into(),
        });
    }

    with_changed(list, id, |ext| {
        if !ext.declined_listings.iter().any(|d| d == store_id) {
            ext.declined_listings.push(store_id.to_string());
        }
        let over = ext.declined_listings.len().saturating_sub(MAX_DECLINED);
        ext.declined_listings.drain(..over);
        if ext
            .store_suggestion
            .as_ref()
            .is_some_and(|s| s.store_id == store_id)
        {
            ext.store_suggestion = None;
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "0123456789abcdef0123456789abcdef";
    const OTHER: &str = "fedcba9876543210fedcba9876543210";
    const STORE: &str = "eimadpbcbfnmbkopoojfekhnkhdbieeh";
    const COPY: &str = "akpkoodohacdmlddblgnaahbbfjplcig";

    fn installed() -> Vec<InstalledExtension> {
        vec![
            InstalledExtension {
                id: ID.into(),
                name: "Dark Reader".into(),
                ..Default::default()
            },
            InstalledExtension {
                id: OTHER.into(),
                name: "SponsorBlock".into(),
                store_id: Some(COPY.into()),
                ..Default::default()
            },
        ]
    }

    fn suggestion(store_id: &str) -> StoreSuggestion {
        StoreSuggestion {
            store_id: store_id.into(),
            same_files: true,
        }
    }

    #[test]
    fn names_match_as_shown() {
        assert!(same_name("Dark Reader", " dark  READER"));
        assert!(!same_name("Dark Reader", "Dark Reader for Chrome"));
        assert!(!same_name("", "Extension"));
        assert!(!same_name("  ", "  "));
    }

    #[test]
    fn listing_slugs_spell_names_as_the_store_does() {
        assert_eq!(listing_slug("Dark Reader").as_deref(), Some("dark-reader"));
        assert_eq!(
            listing_slug("Dark Reader - Dark Mode for Chrome").as_deref(),
            Some("dark-reader-dark-mode-for")
        );
        assert_eq!(
            listing_slug("Dark Theme: Dark Reader for every site").as_deref(),
            Some("dark-theme-dark-reader-fo")
        );
        assert_eq!(
            listing_slug("uBlock Origin Lite").as_deref(),
            Some("ublock-origin-lite")
        );
        assert_eq!(listing_slug("暗黑模式"), None);
    }

    #[test]
    fn search_pages_list_their_listings_once_in_order() {
        let html = r#"<a href="./detail/dark-reader/eimadpbcbfnmbkopoojfekhnkhdbieeh">x</a>
            <a href="https://chromewebstore.google.com/detail/dark-reader-for-chrome/omfeeokgnjnjcgdbppmnijlmdnpafmmp?hl=en">y</a>
            <a href="./detail/dark-reader/eimadpbcbfnmbkopoojfekhnkhdbieeh/reviews">again</a>
            <a href="./detail/not-an-id/eimadpbcbfnmbkopoojfekhnkhdbieez">bad</a>
            <a href="./detail/akpkoodohacdmlddblgnaahbbfjplcig">bare</a>"#;
        let listing = |slug: Option<&str>, id: &str| Listing {
            store_id: id.into(),
            slug: slug.map(Into::into),
        };
        assert_eq!(
            listings(html),
            vec![
                listing(Some("dark-reader"), "eimadpbcbfnmbkopoojfekhnkhdbieeh"),
                listing(
                    Some("dark-reader-for-chrome"),
                    "omfeeokgnjnjcgdbppmnijlmdnpafmmp"
                ),
                listing(None, "akpkoodohacdmlddblgnaahbbfjplcig"),
            ]
        );
        assert!(listings("no listings here").is_empty());
    }

    #[test]
    fn linking_ties_the_listing_and_drops_the_suggestion() {
        let list = with_store_suggestion(&installed(), ID, Some(suggestion(STORE))).unwrap();
        let list = with_store_link(&list, ID, STORE).unwrap();
        assert_eq!(list[0].store_id.as_deref(), Some(STORE));
        assert!(list[0].store_suggestion.is_none());
    }

    #[test]
    fn a_listing_links_once_and_only_by_a_store_id() {
        assert!(with_store_link(&installed(), ID, COPY).is_err());
        assert!(with_store_link(&installed(), ID, "not-an-id").is_err());
        assert!(with_store_link(&installed(), "missing", STORE).is_err());
        assert!(with_store_link(&installed(), OTHER, STORE).is_err());
    }

    #[test]
    fn a_declined_listing_is_not_suggested_again() {
        let list = with_store_suggestion(&installed(), ID, Some(suggestion(STORE))).unwrap();
        let list = with_declined_listing(&list, ID, STORE).unwrap();
        assert!(list[0].store_suggestion.is_none());
        assert_eq!(list[0].declined_listings, vec![STORE.to_string()]);
        let again = with_store_suggestion(&list, ID, Some(suggestion(STORE))).unwrap();
        assert!(again[0].store_suggestion.is_none());
    }

    #[test]
    fn only_unlinked_extensions_without_a_suggestion_are_searched_weekly() {
        let list = installed();
        assert!(wants_search(&list[0], 1_000));
        assert!(!wants_search(&list[1], 1_000));

        let searched = with_store_searched(&list, ID, 1_000).unwrap();
        assert!(!wants_search(&searched[0], 1_000 + SEARCH_AGAIN_AFTER - 1));
        assert!(wants_search(&searched[0], 1_000 + SEARCH_AGAIN_AFTER));
        assert!(wants_search(&searched[0], 10));

        let suggested = with_store_suggestion(&list, ID, Some(suggestion(STORE))).unwrap();
        assert!(!wants_search(&suggested[0], 1_000));
    }

    #[test]
    fn declined_listings_keep_the_latest_few() {
        let mut list = installed();
        for i in 0..MAX_DECLINED + 4 {
            let id: String = format!("{i:032}")
                .bytes()
                .map(|b| char::from(b - b'0' + b'a'))
                .collect();
            list = with_declined_listing(&list, ID, &id).unwrap();
        }
        assert_eq!(list[0].declined_listings.len(), MAX_DECLINED);
        assert!(with_declined_listing(&list, ID, "bad").is_err());
    }
}

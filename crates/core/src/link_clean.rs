//! Stripping tracking parameters from a link.
//!
//! Pure and dependency-light: `url` is used only to check the scheme, never to
//! rebuild the link. Everything here is string surgery on the original text
//! between `?` and `#`.
//!
//! The design is a **list of names to remove**, so every parameter not named
//! here survives untouched. This means that the only way to break a link is
//! to put a wrong name in the table below. The table is therefore deliberately
//! narrow. Short generic names (`si`, `ref`, `cid`, `id`, `t`) are excluded
//! even where they are used for tracking.
//!
//! Fragments are preserved byte for byte. A fragment never reaches the server,
//! so it carries no server-side tracking, and it is load-bearing.

use serde::{Deserialize, Serialize};

/// A namespace rather than a name: Urchin parameters are open-ended, so the
/// whole prefix is safe to take.
const TRACKING_PREFIXES: &[&str] = &["utm_"];

/// Click identifiers and campaign tags that carry attribution and nothing
/// else. Matched case-insensitively, since real sites emit `UTM_Source`.
const TRACKING_PARAMS: &[&str] = &[
    // Advertising click ids.
    "fbclid",
    "gclid",
    "dclid",
    "gclsrc",
    "gbraid",
    "wbraid",
    "msclkid",
    "twclid",
    "ttclid",
    "yclid",
    "igshid",
    "igsh",
    "epik",
    "irclickid",
    "rb_clickid",
    "wickedid",
    // Email and campaign platforms.
    "mc_cid",
    "mc_eid",
    "mkt_tok",
    "vero_id",
    "vero_conv",
    "ml_subscriber",
    "ml_subscriber_hash",
    "oly_anon_id",
    "oly_enc_id",
    "_openstat",
    // HubSpot.
    "__hssc",
    "__hstc",
    "__hsfp",
    "_hsenc",
    "_hsmi",
    "hsctatracking",
    // Analytics linkers and site-wide campaign tags.
    "_ga",
    "_gl",
    "cmpid",
    "ceid",
    "s_cid",
    "spm",
    "at_medium",
    "at_campaign",
    "ns_campaign",
    "ns_source",
    "ns_mchannel",
    "ns_linkname",
];

/// Listed parameters that identify the email recipient.
const SUBSCRIBER_IDS: &[&str] = &[
    "mkt_tok",
    "ml_subscriber",
    "ml_subscriber_hash",
    "vero_id",
    "mc_eid",
    "oly_anon_id",
    "oly_enc_id",
    "_hsenc",
    "_hsmi",
];

const SUBSCRIPTION_PATH_WORDS: &[&str] = &[
    "unsubscribe",
    "optout",
    "opt-out",
    "opt_out",
    "preference",
    "subscription",
    "emailwebview",
];

/// A link with its tracking parameters removed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CleanedLink {
    /// The cleaned link, or the input unchanged when there was nothing to do.
    pub url: String,
    /// Names that were removed. Empty means the link came back untouched.
    pub removed: Vec<String>,
}

/// Whether a parameter name should be removed.
fn is_tracking(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    TRACKING_PREFIXES
        .iter()
        .any(|prefix| lower.starts_with(prefix))
        || TRACKING_PARAMS.iter().any(|known| lower == *known)
}

/// Remove tracking parameters from `href`. Anything unparseable, or not
/// http(s), comes back exactly as it went in with nothing removed.
pub fn clean_link(href: &str) -> CleanedLink {
    clean_except(href, |_| false)
}

/// [`clean_link`] for a link being followed, which keeps the subscriber ids
/// on pages that manage a subscription.
pub fn clean_followed_link(href: &str) -> CleanedLink {
    let manages_subscription = url::Url::parse(href).is_ok_and(|url| {
        let path = url.path().to_ascii_lowercase();
        SUBSCRIPTION_PATH_WORDS
            .iter()
            .any(|word| path.contains(word))
    });
    clean_except(href, |name| {
        manages_subscription
            && SUBSCRIBER_IDS
                .iter()
                .any(|id| name.eq_ignore_ascii_case(id))
    })
}

fn clean_except(href: &str, keep: impl Fn(&str) -> bool) -> CleanedLink {
    let unchanged = || CleanedLink {
        url: href.to_string(),
        removed: Vec::new(),
    };

    // Parsed only to check the scheme.
    match url::Url::parse(href) {
        Ok(parsed) if matches!(parsed.scheme(), "http" | "https") => {}
        _ => return unchanged(),
    }

    // The fragment is found first. A hash-routed URL puts its own `?` inside
    // the fragment.
    let fragment_start = href.find('#');
    let before_fragment = &href[..fragment_start.unwrap_or(href.len())];
    let Some(query_start) = before_fragment.find('?') else {
        return unchanged();
    };
    let query = &before_fragment[query_start + 1..];
    if query.is_empty() {
        return unchanged();
    }

    let mut kept: Vec<&str> = Vec::new();
    let mut removed: Vec<String> = Vec::new();
    for pair in query.split('&') {
        let name = pair.split('=').next().unwrap_or(pair);
        if !name.is_empty() && is_tracking(name) && !keep(name) {
            removed.push(name.to_string());
        } else {
            kept.push(pair);
        }
    }

    if removed.is_empty() {
        return unchanged();
    }

    let mut url = String::with_capacity(href.len());
    url.push_str(&href[..query_start]);
    if !kept.is_empty() {
        url.push('?');
        url.push_str(&kept.join("&"));
    }
    if let Some(fragment) = fragment_start {
        url.push_str(&href[fragment..]);
    }

    CleanedLink { url, removed }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A hash router's query is app state behind the fragment, and the
    /// fragment is promised back byte for byte.
    #[test]
    fn a_query_inside_the_fragment_is_left_alone() {
        let href = "https://app.example.com/#/dashboard?spm=widget3&view=grid";
        let cleaned = clean_link(href);
        assert_eq!(cleaned.url, href);
        assert!(cleaned.removed.is_empty());
    }

    #[test]
    fn a_real_query_is_cleaned_even_when_the_fragment_has_one_too() {
        let cleaned = clean_link("https://example.com/p?utm_source=x&a=1#/route?spm=keepme");
        assert_eq!(cleaned.url, "https://example.com/p?a=1#/route?spm=keepme");
        assert_eq!(cleaned.removed, vec!["utm_source"]);
    }

    fn clean(href: &str) -> String {
        clean_link(href).url
    }

    #[test]
    fn a_followed_link_keeps_subscriber_ids_on_subscription_pages_only() {
        let unsubscribe = "https://go.example.com/UnsubscribePage.html?mkt_tok=AbC&utm_source=mail";
        assert_eq!(
            clean_followed_link(unsubscribe).url,
            "https://go.example.com/UnsubscribePage.html?mkt_tok=AbC"
        );
        for page in [
            "https://news.example/email-preferences?_hsenc=x&_hsmi=y",
            "https://news.example/hs/manage-preferences/unsubscribe?_hsenc=x",
            "https://news.example/emailWebview?mkt_tok=x",
            "https://news.example/opt-out?mc_eid=x",
            "https://news.example/subscription/center?ml_subscriber=1&ml_subscriber_hash=h",
        ] {
            assert_eq!(clean_followed_link(page).url, page, "{page}");
        }

        assert_eq!(
            clean_followed_link("https://news.example/story?mkt_tok=AbC&id=4").url,
            "https://news.example/story?id=4"
        );
        assert_eq!(
            clean_link(unsubscribe).url,
            "https://go.example.com/UnsubscribePage.html"
        );
    }

    #[test]
    fn a_link_with_nothing_to_strip_comes_back_byte_identical() {
        for href in [
            "https://example.com/a/b?q=hello+world&page=2#frag",
            "https://example.com/search?q=a%20b&x=%2Fpath",
            "https://user:pw@example.com:8443/p?keep=1",
            "https://example.com/",
        ] {
            let cleaned = clean_link(href);
            assert_eq!(cleaned.url, href, "unchanged for {href}");
            assert!(cleaned.removed.is_empty(), "nothing removed for {href}");
        }
    }

    #[test]
    fn the_utm_namespace_goes_whole() {
        let cleaned = clean_link("https://example.com/p?utm_source=x&utm_whatever=y&keep=1");
        assert_eq!(cleaned.url, "https://example.com/p?keep=1");
        assert_eq!(cleaned.removed, vec!["utm_source", "utm_whatever"]);
    }

    #[test]
    fn named_click_identifiers_go() {
        assert_eq!(
            clean("https://example.com/p?fbclid=abc&gclid=def&id=7"),
            "https://example.com/p?id=7"
        );
    }

    #[test]
    fn names_are_matched_without_regard_to_case() {
        assert_eq!(
            clean("https://example.com/p?UTM_Source=x&FBCLID=y&Keep=1"),
            "https://example.com/p?Keep=1"
        );
    }

    #[test]
    fn surviving_parameters_keep_their_order_and_spelling() {
        assert_eq!(
            clean("https://example.com/p?b=2&utm_source=x&a=1&c=%20"),
            "https://example.com/p?b=2&a=1&c=%20"
        );
    }

    #[test]
    fn the_question_mark_goes_when_nothing_survives() {
        assert_eq!(
            clean("https://example.com/p?utm_source=x"),
            "https://example.com/p"
        );
        assert_eq!(
            clean("https://example.com/p?utm_source=x#frag"),
            "https://example.com/p#frag"
        );
    }

    #[test]
    fn a_repeated_tracking_key_is_removed_every_time() {
        assert_eq!(
            clean("https://example.com/p?utm_source=a&keep=1&utm_source=b"),
            "https://example.com/p?keep=1"
        );
    }

    #[test]
    fn a_tracking_key_with_no_value_still_goes() {
        assert_eq!(
            clean("https://example.com/p?utm_source&keep"),
            "https://example.com/p?keep"
        );
    }

    #[test]
    fn the_fragment_is_preserved_exactly() {
        assert_eq!(
            clean("https://example.com/p?utm_source=x&keep=1#:~:text=hello%20there"),
            "https://example.com/p?keep=1#:~:text=hello%20there"
        );
    }

    #[test]
    fn links_this_does_not_understand_pass_straight_through() {
        for href in [
            "mailto:someone@example.com?subject=utm_source",
            "javascript:void(0)",
            "not a url at all",
            "",
        ] {
            let cleaned = clean_link(href);
            assert_eq!(cleaned.url, href);
            assert!(cleaned.removed.is_empty());
        }
    }

    #[test]
    fn content_bearing_parameters_are_never_touched() {
        // The short generic names deliberately left out of the table, plus the
        // signature parameters a presigned URL cannot survive without.
        let video = "https://www.youtube.com/watch?v=abc123&t=42&si=xyz";
        assert_eq!(clean(video), video);

        let signed = "https://s3.example.com/o?X-Amz-Signature=deadbeef&X-Amz-Expires=900";
        assert_eq!(clean(signed), signed);
    }
}

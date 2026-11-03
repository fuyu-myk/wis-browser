//! Which site a page belongs to, and a registrable-site approximation (eTLD+1
//! without a public-suffix list).

use std::borrow::Cow;

use url::Url;

/// The URL whose site a page belongs to: `url` itself, or for a `blob:` URL
/// one on the http(s) origin that made it, where only the scheme, host and
/// port mean anything. `None` for a `blob:` URL from anywhere else.
pub fn site_url(url: &Url) -> Option<Cow<'_, Url>> {
    if url.scheme() != "blob" {
        return Some(Cow::Borrowed(url));
    }

    Url::parse(url.path())
        .ok()
        .filter(|inner| matches!(inner.scheme(), "http" | "https"))
        .map(Cow::Owned)
}

/// The last two host labels, or three when the suffix looks like a ccSLD
/// ("co.uk", "com.au"), so `mail.google.com` and `accounts.google.com` are one
/// site while `a.co.uk` and `b.co.uk` are not. IP hosts are returned whole.
pub fn registrable_domain(host: &str) -> String {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    if host.chars().all(|c| c.is_ascii_digit() || c == '.') || host.starts_with('[') {
        return host;
    }

    let labels: Vec<&str> = host.split('.').filter(|l| !l.is_empty()).collect();
    if labels.len() <= 2 {
        return labels.join(".");
    }

    const CC_SECOND: &[&str] = &["co", "com", "org", "net", "ac", "gov", "edu", "ne", "or"];
    let tld = labels[labels.len() - 1];
    let second = labels[labels.len() - 2];
    let take = if tld.len() == 2 && CC_SECOND.contains(&second) {
        3
    } else {
        2
    };

    labels[labels.len() - take.min(labels.len())..].join(".")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subdomains_share_their_registrable_site() {
        assert_eq!(registrable_domain("mail.google.com"), "google.com");
        assert_eq!(registrable_domain("WWW.Example.COM."), "example.com");
        assert_eq!(registrable_domain("news.bbc.co.uk"), "bbc.co.uk");
        assert_eq!(registrable_domain("192.168.1.10"), "192.168.1.10");
        assert_eq!(registrable_domain("localhost"), "localhost");
    }

    fn site_of(url: &str) -> Option<String> {
        site_url(&Url::parse(url).unwrap()).map(|site| site.to_string())
    }

    #[test]
    fn a_page_belongs_to_its_own_url() {
        assert_eq!(
            site_of("https://news.example/a?b"),
            Some("https://news.example/a?b".to_string())
        );
        assert_eq!(site_of("about:blank"), Some("about:blank".to_string()));
    }

    #[test]
    fn a_blob_page_belongs_to_the_web_page_that_made_it() {
        assert_eq!(
            site_of("blob:https://www.video.example/0b5f7c1e"),
            Some("https://www.video.example/0b5f7c1e".to_string())
        );
        assert_eq!(site_of("blob:null/0b5f7c1e"), None);
        assert_eq!(site_of("blob:ws://video.example/0b5f7c1e"), None);
        assert_eq!(site_of("blob:blob:https://video.example/0b5f7c1e"), None);
    }
}

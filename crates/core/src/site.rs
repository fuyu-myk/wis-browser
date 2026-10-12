//! Registrable-site approximation (eTLD+1 without a public-suffix list).

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
}

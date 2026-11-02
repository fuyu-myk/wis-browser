//! Cookies a download's redirect chain sets, for a download Wis fetches
//! itself. They live for that fetch and never reach the engine's store.

use url::Url;

/// What one fetch's redirect chain set over HTTPS, each cookie sent back only
/// to the host that set it, and over HTTPS only when it is secure. A plaintext
/// hop sets nothing, so it cannot stand in for a secure cookie.
#[derive(Debug, Default)]
pub struct ChainCookies(Vec<ChainCookie>);

#[derive(Debug)]
struct ChainCookie {
    host: String,
    name: String,
    /// `None` once the chain expired it.
    value: Option<String>,
    secure: bool,
}

impl ChainCookies {
    /// Take in a `Set-Cookie` value the response from `url` carried.
    pub fn set(&mut self, url: &Url, set_cookie: &str) {
        let Some(host) = url.host_str().filter(|_| url.scheme() == "https") else {
            return;
        };
        let mut parts = set_cookie.split(';');
        let Some((name, value)) = parts.next().and_then(|pair| pair.split_once('=')) else {
            return;
        };
        let name = name.trim();
        if name.is_empty() {
            return;
        }

        let mut secure = false;
        let mut expired = false;
        for attribute in parts {
            let (key, value) = attribute.split_once('=').unwrap_or((attribute, ""));
            let key = key.trim();
            if key.eq_ignore_ascii_case("secure") {
                secure = true;
            } else if key.eq_ignore_ascii_case("max-age") {
                expired = value.trim().parse::<i64>().is_ok_and(|age| age <= 0);
            }
        }

        self.0
            .retain(|cookie| !(cookie.host == host && cookie.name == name));
        self.0.push(ChainCookie {
            host: host.to_string(),
            name: name.to_string(),
            value: (!expired).then(|| value.trim().to_string()),
            secure,
        });
    }

    /// The `Cookie` header for `url`: the engine's pairs for it, with any the
    /// chain set on this host in their place. `None` when nothing is sent.
    pub fn header(&self, url: &Url, engine: &[(String, String)]) -> Option<String> {
        let ours: Vec<&ChainCookie> = self
            .0
            .iter()
            .filter(|cookie| Some(cookie.host.as_str()) == url.host_str())
            .filter(|cookie| !cookie.secure || url.scheme() == "https")
            .collect();
        let pairs: Vec<String> = engine
            .iter()
            .filter(|(name, _)| !ours.iter().any(|cookie| cookie.name == *name))
            .map(|(name, value)| (name.as_str(), value.as_str()))
            .chain(
                ours.iter()
                    .filter_map(|cookie| Some((cookie.name.as_str(), cookie.value.as_deref()?))),
            )
            .filter(|(name, value)| sendable(name) && sendable(value))
            .map(|(name, value)| match name {
                "" => value.to_string(),
                _ => format!("{name}={value}"),
            })
            .collect();

        (!pairs.is_empty()).then(|| pairs.join("; "))
    }
}

/// Text that can stand in a `Cookie` header without ending a pair early.
fn sendable(text: &str) -> bool {
    !text.contains(|c: char| c.is_control() || c == ';')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(raw: &str) -> Url {
        Url::parse(raw).unwrap()
    }

    fn engine(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(name, value)| (name.to_string(), value.to_string()))
            .collect()
    }

    #[test]
    fn a_chain_cookie_goes_back_only_to_its_host() {
        let mut chain = ChainCookies::default();
        chain.set(
            &at("https://a.example.com/login"),
            "token=abc; Path=/; HttpOnly",
        );
        let header = chain.header(&at("https://a.example.com/file"), &[]);
        assert_eq!(header.as_deref(), Some("token=abc"));
        assert_eq!(chain.header(&at("https://b.example.com/file"), &[]), None);
    }

    #[test]
    fn a_chain_cookie_replaces_the_engine_one_of_its_name() {
        let mut chain = ChainCookies::default();
        chain.set(&at("https://a.example.com/"), "sid=new");
        let header = chain.header(
            &at("https://a.example.com/file"),
            &engine(&[("sid", "old"), ("lang", "en")]),
        );
        assert_eq!(header.as_deref(), Some("lang=en; sid=new"));
    }

    #[test]
    fn an_expired_chain_cookie_withholds_the_engine_one() {
        let mut chain = ChainCookies::default();
        chain.set(&at("https://a.example.com/"), "sid=; Max-Age=0");
        let header = chain.header(
            &at("https://a.example.com/file"),
            &engine(&[("sid", "old"), ("lang", "en")]),
        );
        assert_eq!(header.as_deref(), Some("lang=en"));
    }

    #[test]
    fn a_secure_chain_cookie_is_not_sent_over_http() {
        let mut chain = ChainCookies::default();
        chain.set(&at("https://a.example.com/"), "sid=1; Secure");
        assert_eq!(chain.header(&at("http://a.example.com/file"), &[]), None);
    }

    #[test]
    fn a_later_set_replaces_an_earlier_one() {
        let mut chain = ChainCookies::default();
        chain.set(&at("https://a.example.com/"), "sid=1");
        chain.set(&at("https://a.example.com/next"), "sid=2");
        let header = chain.header(&at("https://a.example.com/file"), &[]);
        assert_eq!(header.as_deref(), Some("sid=2"));
    }

    #[test]
    fn an_unsendable_pair_is_skipped_and_a_nameless_one_is_sent_bare() {
        let chain = ChainCookies::default();
        let header = chain.header(
            &at("https://a.example.com/file"),
            &engine(&[("bad", "a\u{0}b"), ("", "bare"), ("ok", "1")]),
        );
        assert_eq!(header.as_deref(), Some("bare; ok=1"));
    }

    #[test]
    fn a_cookie_set_over_http_is_ignored() {
        let mut chain = ChainCookies::default();
        chain.set(&at("http://a.example.com/"), "sid=attacker");
        let header = chain.header(
            &at("https://a.example.com/file"),
            &engine(&[("sid", "victim")]),
        );
        assert_eq!(header.as_deref(), Some("sid=victim"));
    }

    #[test]
    fn an_unreadable_set_cookie_is_ignored() {
        let mut chain = ChainCookies::default();
        chain.set(&at("https://a.example.com/"), "no value here");
        assert_eq!(chain.header(&at("https://a.example.com/file"), &[]), None);
    }
}

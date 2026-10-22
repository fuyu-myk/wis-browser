//! CNAME uncloaking: a host whose DNS chain passes through a listed tracker
//! is refused like the tracker. Only a direct route sees the chain; Tor and
//! SOCKS upstreams resolve names themselves.

use std::collections::VecDeque;
use std::sync::{Mutex, MutexGuard, PoisonError};

const REMEMBERED: usize = 256;

/// One workspace's pages being opened, which uBO leaves alone so that email
/// links and landing pages run through tracker-hosted subdomains still open,
/// and the hosts refused for their aliases, which the site panel counts.
#[derive(Default)]
pub struct Cloaking {
    pages: Mutex<RecentHosts>,
    uncloaked: Mutex<RecentHosts>,
}

impl Cloaking {
    /// Before a tab, Peek or panel navigates its main frame to `url`.
    pub fn opening_page(&self, url: &str) {
        if let Some(host) = url::Url::parse(url)
            .ok()
            .and_then(|url| url.host_str().map(str::to_owned))
        {
            lock(&self.pages).insert(host);
        }
    }

    pub fn is_page(&self, host: &str) -> bool {
        lock(&self.pages).contains(host)
    }

    pub fn record_uncloaked(&self, host: &str) {
        lock(&self.uncloaked).insert(host.to_owned());
    }

    pub fn was_uncloaked(&self, host: &str) -> bool {
        lock(&self.uncloaked).contains(host)
    }

    /// In place, as the workspace's live proxy holds this same state.
    pub fn clear(&self) {
        lock(&self.pages).hosts.clear();
        lock(&self.uncloaked).hosts.clear();
    }
}

fn lock(hosts: &Mutex<RecentHosts>) -> MutexGuard<'_, RecentHosts> {
    hosts.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The most recent hosts, up to `capacity`.
struct RecentHosts {
    hosts: VecDeque<String>,
    capacity: usize,
}

impl Default for RecentHosts {
    fn default() -> Self {
        Self::new(REMEMBERED)
    }
}

impl RecentHosts {
    fn new(capacity: usize) -> Self {
        Self {
            hosts: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    fn insert(&mut self, host: String) {
        self.hosts.retain(|known| *known != host);
        if self.hosts.len() == self.capacity {
            self.hosts.pop_front();
        }
        self.hosts.push_back(host);
    }

    fn contains(&self, host: &str) -> bool {
        self.hosts.iter().any(|known| known == host)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_oldest_host_goes_first_and_a_revisit_counts_as_new() {
        let mut hosts = RecentHosts::new(2);
        hosts.insert("a.test".into());
        hosts.insert("b.test".into());
        hosts.insert("a.test".into());
        hosts.insert("c.test".into());
        assert!(hosts.contains("a.test"));
        assert!(!hosts.contains("b.test"));
        assert!(hosts.contains("c.test"));
    }

    #[test]
    fn clearing_forgets_pages_and_refusals() {
        let cloaking = Cloaking::default();
        cloaking.opening_page("https://go.shop.test/");
        cloaking.record_uncloaked("metrics.shop.test");
        cloaking.clear();
        assert!(!cloaking.is_page("go.shop.test"));
        assert!(!cloaking.was_uncloaked("metrics.shop.test"));
    }

    #[test]
    fn a_page_is_known_by_its_host() {
        let cloaking = Cloaking::default();
        cloaking.opening_page("https://landing.example.test/offer?id=1");
        cloaking.opening_page("not a url");
        assert!(cloaking.is_page("landing.example.test"));
        assert!(!cloaking.is_page("example.test"));
    }
}

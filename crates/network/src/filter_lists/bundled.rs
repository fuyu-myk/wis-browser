//! Bundled minimal tracker filter list, used as the offline fallback when
//! the EasyPrivacy cache is absent or stale and the network is unreachable.
//!
//! Written in Adblock Plus syntax so it can be parsed by the `adblock` crate
//! used by [`crate::blocker::TrackerBlocker`].

pub const DEFAULT_TRACKER_LIST: &str = r#"
! Title: Wis Default Tracker List
! Description: Minimal bundled list of common trackers blocked out-of-the-box.
! Source: hand-curated offline fallback.

! ---- Google ----
||doubleclick.net^
||googletagmanager.com^
||googletagservices.com^
||google-analytics.com^
||ssl.google-analytics.com^
||analytics.google.com^
||adservice.google.com^
||adservice.google.co.uk^
||googleadservices.com^
||googlesyndication.com^

! ---- Meta / Facebook ----
||facebook.com/tr^
||connect.facebook.net^
||graph.facebook.com$third-party

! ---- Microsoft / LinkedIn ----
||clarity.ms^
||bat.bing.com^
||px.ads.linkedin.com^

! ---- Generic analytics / tag managers ----
||scorecardresearch.com^
||quantserve.com^
||hotjar.com^
||segment.io^
||segment.com^
||mixpanel.com^
||amplitude.com^
||fullstory.com^
||logrocket.com^
||heap.io^

! ---- Ad networks ----
||outbrain.com^
||taboola.com^
||criteo.com^
||adsrvr.org^
||adnxs.com^
||rubiconproject.com^
||pubmatic.com^
||openx.net^
"#;

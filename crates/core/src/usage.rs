//! Site usage: active time credited per workspace, day and site.

use std::time::Duration;

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::ids::WorkspaceId;
use crate::site::registrable_domain;

/// The first day of a usage week.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WeekStart {
    /// The region's convention.
    #[default]
    Auto,
    Monday,
    Sunday,
}

/// Input within this long counts as presence and confirms held time.
pub const PRESENT: Duration = Duration::from_secs(2);

/// The site a page's time counts toward; only http(s) pages count.
pub fn site_of(url: &Url) -> Option<String> {
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }

    let site = registrable_domain(url.host_str()?);
    (!site.is_empty()).then_some(site)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Focus {
    pub workspace: WorkspaceId,
    pub site: String,
    pub audible: bool,
}

/// What the browser looks like at one tick.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Observation {
    pub day: NaiveDate,
    /// Local hour of the day, 0 to 23.
    pub hour: u8,
    /// Time since the last keyboard or pointer input anywhere; `None` where the
    /// platform cannot tell.
    pub idle: Option<Duration>,
    /// The site in the focused pane of the focused Wis window.
    pub focused: Option<Focus>,
    /// An audible site in the focused pane of a visible Wis window.
    pub playing: Option<Focus>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Credit {
    pub workspace: WorkspaceId,
    pub day: NaiveDate,
    pub hour: u8,
    pub site: String,
    pub seconds: u32,
}

/// Credits one second per active tick. Seconds without input are held until
/// input confirms them and dropped once idle passes the timeout, so walking
/// away adds nothing. Audible sites count without input.
#[derive(Debug)]
pub struct UsageMeter {
    timeout: Duration,
    held: Vec<Credit>,
    last_idle: Option<Duration>,
}

impl UsageMeter {
    pub fn new(timeout: Duration) -> Self {
        Self {
            timeout,
            held: Vec::new(),
            last_idle: None,
        }
    }

    pub fn set_timeout(&mut self, timeout: Duration) {
        self.timeout = timeout;
    }

    pub fn reset(&mut self) {
        self.held.clear();
        self.last_idle = None;
    }

    pub fn retain(&mut self, keep: impl Fn(&Credit) -> bool) {
        self.held.retain(|credit| keep(credit));
    }

    /// Credits ready to store after one tick. The focused site counts while
    /// it plays or takes input; otherwise a playing visible window counts, and
    /// failing that the focused site is held.
    pub fn tick(&mut self, observation: &Observation) -> Vec<Credit> {
        let mut ready = Vec::new();
        let input_since_last = matches!(
            (observation.idle, self.last_idle),
            (Some(now), Some(before)) if now < before
        );
        self.last_idle = observation.idle;
        let present = input_since_last || observation.idle.is_none_or(|idle| idle <= PRESENT);
        let timed_out = observation.idle.is_some_and(|idle| idle >= self.timeout);
        if timed_out {
            self.held.clear();
        } else if present {
            for credit in self.held.drain(..) {
                add(&mut ready, credit);
            }
        }

        let second = |focus: &Focus| Credit {
            workspace: focus.workspace,
            day: observation.day,
            hour: observation.hour,
            site: focus.site.clone(),
            seconds: 1,
        };
        let focused = observation.focused.as_ref();
        let playing = observation.playing.as_ref().filter(|focus| focus.audible);
        if let Some(focus) = focused.filter(|focus| focus.audible || present) {
            add(&mut ready, second(focus));
        } else if let Some(focus) = playing {
            add(&mut ready, second(focus));
        } else if let Some(focus) = focused.filter(|_| !timed_out) {
            add(&mut self.held, second(focus));
        }

        ready
    }
}

/// A meter plus the credits it produced that are not stored yet.
#[derive(Debug)]
pub struct UsageLedger {
    meter: UsageMeter,
    pending: Vec<Credit>,
}

impl UsageLedger {
    pub fn new(timeout: Duration) -> Self {
        Self {
            meter: UsageMeter::new(timeout),
            pending: Vec::new(),
        }
    }

    pub fn set_timeout(&mut self, timeout: Duration) {
        self.meter.set_timeout(timeout);
    }

    pub fn record(&mut self, observation: &Observation) {
        let credits = self.meter.tick(observation);
        merge(&mut self.pending, credits);
    }

    /// Tracking is off, so nothing held carries over to when it is back on.
    pub fn pause(&mut self) {
        self.meter.reset();
    }

    pub fn take(&mut self) -> Vec<Credit> {
        std::mem::take(&mut self.pending)
    }

    /// Puts back credits that could not be stored.
    pub fn restore(&mut self, credits: Vec<Credit>) {
        merge(&mut self.pending, credits);
    }

    /// Drops every held and pending credit `cleared` covers.
    pub fn forget(&mut self, cleared: impl Fn(&Credit) -> bool) {
        self.pending.retain(|credit| !cleared(credit));
        self.meter.retain(|credit| !cleared(credit));
    }
}

/// Folds `more` into `credits`, one entry per workspace, day and site.
pub fn merge(credits: &mut Vec<Credit>, more: impl IntoIterator<Item = Credit>) {
    for credit in more {
        add(credits, credit);
    }
}

fn add(credits: &mut Vec<Credit>, credit: Credit) {
    match credits.iter_mut().find(|c| {
        c.workspace == credit.workspace
            && c.day == credit.day
            && c.hour == credit.hour
            && c.site == credit.site
    }) {
        Some(existing) => existing.seconds += credit.seconds,
        None => credits.push(credit),
    }
}

/// Stored time for one local day, hour and site.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UsageRow {
    pub day: NaiveDate,
    pub hour: u8,
    pub site: String,
    pub seconds: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SiteTime {
    pub site: String,
    pub seconds: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DayTime {
    pub day: NaiveDate,
    pub seconds: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UsageSummary {
    pub total_seconds: i64,
    /// Busiest first.
    pub sites: Vec<SiteTime>,
    /// Seconds per local hour, summed over the range.
    pub hours: Vec<i64>,
    /// Every day of the range, quiet days included.
    pub days: Vec<DayTime>,
}

pub fn summarize(rows: &[UsageRow], from: NaiveDate, to: NaiveDate) -> UsageSummary {
    let mut sites: Vec<SiteTime> = Vec::new();
    let mut hours = vec![0i64; 24];
    let mut days: Vec<DayTime> = from
        .iter_days()
        .take_while(|day| *day <= to)
        .map(|day| DayTime { day, seconds: 0 })
        .collect();

    for row in rows
        .iter()
        .filter(|row| row.day >= from && row.day <= to && row.hour < 24 && row.seconds > 0)
    {
        hours[usize::from(row.hour)] += row.seconds;
        if let Some(day) = days.iter_mut().find(|d| d.day == row.day) {
            day.seconds += row.seconds;
        }
        match sites.iter_mut().find(|s| s.site == row.site) {
            Some(site) => site.seconds += row.seconds,
            None => sites.push(SiteTime {
                site: row.site.clone(),
                seconds: row.seconds,
            }),
        }
    }
    sites.sort_by(|a, b| b.seconds.cmp(&a.seconds).then_with(|| a.site.cmp(&b.site)));

    UsageSummary {
        total_seconds: hours.iter().sum(),
        sites,
        hours,
        days,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TIMEOUT: Duration = Duration::from_secs(120);

    fn day() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, 11).unwrap()
    }

    fn focus(ws: WorkspaceId, site: &str, audible: bool) -> Focus {
        Focus {
            workspace: ws,
            site: site.to_string(),
            audible,
        }
    }

    fn seen(idle: u64, focused: Option<Focus>, playing: Option<Focus>) -> Observation {
        Observation {
            day: day(),
            hour: 10,
            idle: Some(Duration::from_secs(idle)),
            focused,
            playing,
        }
    }

    fn total(credits: &[Credit], site: &str) -> u32 {
        credits
            .iter()
            .filter(|c| c.site == site)
            .map(|c| c.seconds)
            .sum()
    }

    #[test]
    fn only_web_pages_have_a_site() {
        let site = |raw: &str| site_of(&Url::parse(raw).unwrap());
        assert_eq!(
            site("https://mail.google.com/inbox").as_deref(),
            Some("google.com")
        );
        assert_eq!(
            site("http://www.example.com/").as_deref(),
            Some("example.com")
        );
        assert_eq!(site("about:settings"), None);
        assert_eq!(site("file:///tmp/a.html"), None);
    }

    fn row(d: u32, hour: u8, site: &str, seconds: i64) -> UsageRow {
        UsageRow {
            day: NaiveDate::from_ymd_opt(2026, 10, d).unwrap(),
            hour,
            site: site.to_string(),
            seconds,
        }
    }

    #[test]
    fn a_summary_totals_sites_hours_and_every_day_of_the_range() {
        let from = NaiveDate::from_ymd_opt(2026, 10, 5).unwrap();
        let to = NaiveDate::from_ymd_opt(2026, 10, 7).unwrap();
        let rows = [
            row(5, 9, "a.example", 60),
            row(5, 9, "b.example", 600),
            row(7, 21, "a.example", 120),
            row(8, 1, "late.example", 999),
            row(4, 1, "early.example", 999),
        ];

        let summary = summarize(&rows, from, to);

        assert_eq!(summary.total_seconds, 780);
        assert_eq!(
            summary.sites,
            vec![
                SiteTime {
                    site: "b.example".to_string(),
                    seconds: 600
                },
                SiteTime {
                    site: "a.example".to_string(),
                    seconds: 180
                },
            ]
        );
        assert_eq!(summary.hours.len(), 24);
        assert_eq!((summary.hours[9], summary.hours[21]), (660, 120));
        assert_eq!(
            summary.days.iter().map(|d| d.seconds).collect::<Vec<_>>(),
            vec![660, 0, 120]
        );
    }

    #[test]
    fn credits_in_different_hours_stay_apart() {
        let ws = WorkspaceId::new();
        let mut meter = UsageMeter::new(TIMEOUT);
        let reading = Some(focus(ws, "a.example", false));

        let mut credits = meter.tick(&seen(0, reading.clone(), None));
        credits.extend(meter.tick(&Observation {
            hour: 11,
            ..seen(0, reading, None)
        }));

        assert_eq!(credits.len(), 2);
        assert_eq!((credits[0].hour, credits[1].hour), (10, 11));
    }

    #[test]
    fn active_input_on_the_focused_site_counts_at_once() {
        let ws = WorkspaceId::new();
        let mut meter = UsageMeter::new(TIMEOUT);

        let credits = meter.tick(&seen(0, Some(focus(ws, "a.example", false)), None));

        assert_eq!(
            credits,
            vec![Credit {
                workspace: ws,
                day: day(),
                hour: 10,
                site: "a.example".to_string(),
                seconds: 1,
            }]
        );
    }

    #[test]
    fn quiet_reading_is_held_then_confirmed_by_input() {
        let ws = WorkspaceId::new();
        let mut meter = UsageMeter::new(TIMEOUT);
        let reading = Some(focus(ws, "a.example", false));

        let mut credits = Vec::new();
        for idle in 3..=60 {
            credits.extend(meter.tick(&seen(idle, reading.clone(), None)));
        }
        assert!(credits.is_empty());

        credits.extend(meter.tick(&seen(0, reading, None)));
        assert_eq!(total(&credits, "a.example"), 59);
    }

    #[test]
    fn walking_away_drops_the_held_stretch() {
        let ws = WorkspaceId::new();
        let mut meter = UsageMeter::new(TIMEOUT);
        let reading = Some(focus(ws, "a.example", false));

        let mut credits = Vec::new();
        for idle in 3..=200 {
            credits.extend(meter.tick(&seen(idle, reading.clone(), None)));
        }
        credits.extend(meter.tick(&seen(0, reading, None)));

        assert_eq!(total(&credits, "a.example"), 1);
    }

    #[test]
    fn an_audible_focused_site_counts_without_input() {
        let ws = WorkspaceId::new();
        let mut meter = UsageMeter::new(TIMEOUT);

        let credits = meter.tick(&seen(600, Some(focus(ws, "video.example", true)), None));

        assert_eq!(total(&credits, "video.example"), 1);
    }

    #[test]
    fn a_visible_playing_window_counts_while_another_app_is_in_front() {
        let ws = WorkspaceId::new();
        let mut meter = UsageMeter::new(TIMEOUT);

        let credits = meter.tick(&seen(0, None, Some(focus(ws, "video.example", true))));

        assert_eq!(total(&credits, "video.example"), 1);
    }

    #[test]
    fn the_focused_site_wins_over_another_playing_window() {
        let ws = WorkspaceId::new();
        let mut meter = UsageMeter::new(TIMEOUT);

        let credits = meter.tick(&seen(
            0,
            Some(focus(ws, "docs.example", false)),
            Some(focus(ws, "music.example", true)),
        ));

        assert_eq!(total(&credits, "docs.example"), 1);
        assert_eq!(total(&credits, "music.example"), 0);
    }

    #[test]
    fn a_playing_window_counts_while_the_idle_focused_site_waits() {
        let ws = WorkspaceId::new();
        let mut meter = UsageMeter::new(TIMEOUT);

        let credits = meter.tick(&seen(
            30,
            Some(focus(ws, "docs.example", false)),
            Some(focus(ws, "video.example", true)),
        ));
        let later = meter.tick(&seen(0, Some(focus(ws, "docs.example", false)), None));

        assert_eq!(total(&credits, "video.example"), 1);
        assert_eq!(total(&later, "docs.example"), 1);
    }

    #[test]
    fn input_sampled_late_still_confirms_held_time() {
        let ws = WorkspaceId::new();
        let mut meter = UsageMeter::new(TIMEOUT);
        let reading = Some(focus(ws, "a.example", false));
        for idle in 3..=60 {
            meter.tick(&seen(idle, reading.clone(), None));
        }

        let credits = meter.tick(&seen(3, reading, None));

        assert_eq!(total(&credits, "a.example"), 59);
    }

    #[test]
    fn a_ledger_forgets_held_and_pending_time_for_a_cleared_workspace() {
        let (a, b) = (WorkspaceId::new(), WorkspaceId::new());
        let mut ledger = UsageLedger::new(TIMEOUT);
        ledger.record(&seen(0, Some(focus(a, "a.example", false)), None));
        ledger.record(&seen(0, Some(focus(b, "b.example", false)), None));
        for idle in 3..=10 {
            ledger.record(&seen(idle, Some(focus(a, "a.example", false)), None));
        }

        ledger.forget(|credit| credit.workspace == a);
        ledger.record(&seen(0, None, None));

        let stored = ledger.take();
        assert_eq!(total(&stored, "a.example"), 0);
        assert_eq!(total(&stored, "b.example"), 1);
    }

    #[test]
    fn pausing_drops_held_time() {
        let ws = WorkspaceId::new();
        let mut ledger = UsageLedger::new(TIMEOUT);
        for idle in 3..=30 {
            ledger.record(&seen(idle, Some(focus(ws, "a.example", false)), None));
        }

        ledger.pause();
        ledger.record(&seen(0, None, None));

        assert!(ledger.take().is_empty());
    }

    #[test]
    fn restored_credits_are_stored_next_time() {
        let ws = WorkspaceId::new();
        let mut ledger = UsageLedger::new(TIMEOUT);
        ledger.record(&seen(0, Some(focus(ws, "a.example", false)), None));
        let failed = ledger.take();

        ledger.restore(failed);
        ledger.record(&seen(0, Some(focus(ws, "a.example", false)), None));

        assert_eq!(total(&ledger.take(), "a.example"), 2);
    }

    #[test]
    fn nothing_counts_with_wis_in_the_background_and_nothing_playing() {
        let mut meter = UsageMeter::new(TIMEOUT);

        assert!(meter.tick(&seen(0, None, None)).is_empty());
    }

    #[test]
    fn held_time_confirmed_after_switching_apps_still_counts() {
        let ws = WorkspaceId::new();
        let mut meter = UsageMeter::new(TIMEOUT);
        for idle in 3..=10 {
            meter.tick(&seen(idle, Some(focus(ws, "a.example", false)), None));
        }

        let credits = meter.tick(&seen(0, None, None));

        assert_eq!(total(&credits, "a.example"), 8);
    }

    #[test]
    fn without_an_idle_signal_focused_time_counts() {
        let ws = WorkspaceId::new();
        let mut meter = UsageMeter::new(TIMEOUT);
        let observation = Observation {
            idle: None,
            ..seen(0, Some(focus(ws, "a.example", false)), None)
        };

        assert_eq!(total(&meter.tick(&observation), "a.example"), 1);
    }

    #[test]
    fn a_shorter_timeout_applies_to_time_already_held() {
        let ws = WorkspaceId::new();
        let mut meter = UsageMeter::new(TIMEOUT);
        let reading = Some(focus(ws, "a.example", false));
        for idle in 3..=60 {
            meter.tick(&seen(idle, reading.clone(), None));
        }

        meter.set_timeout(Duration::from_secs(30));
        let credits = meter.tick(&seen(61, reading.clone(), None));
        let after = meter.tick(&seen(0, reading, None));

        assert!(credits.is_empty());
        assert_eq!(total(&after, "a.example"), 1);
    }
}

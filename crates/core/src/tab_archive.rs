//! Closing and archiving tabs left idle into Recently closed.

use std::collections::HashSet;

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use crate::error::WisResult;
use crate::ids::TabId;
use crate::state::BrowserState;
use crate::tab::Tab;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TabArchiveAfter {
    #[default]
    Off,
    TwelveHours,
    OneDay,
    SevenDays,
    ThirtyDays,
}

impl TabArchiveAfter {
    pub fn idle_for(self) -> Option<Duration> {
        match self {
            Self::Off => None,
            Self::TwelveHours => Some(Duration::hours(12)),
            Self::OneDay => Some(Duration::days(1)),
            Self::SevenDays => Some(Duration::days(7)),
            Self::ThirtyDays => Some(Duration::days(30)),
        }
    }
}

/// A tab closed for being idle, with its place in its workspace's tab order.
pub struct Archived {
    pub tab: Tab,
    pub position: Option<usize>,
}

impl BrowserState {
    /// Touches the tabs `before` showed (a workspace's active tab or a split
    /// pane) that this state no longer does, so idle time runs from when a
    /// tab was last on screen, whatever the transition.
    pub fn with_hidden_tabs_touched(self, before: &BrowserState) -> Self {
        let shown = self.shown_tabs();
        let hidden: HashSet<TabId> = before
            .shown_tabs()
            .into_iter()
            .filter(|id| !shown.contains(id))
            .collect();
        if hidden.is_empty() || !self.tabs.iter().any(|tab| hidden.contains(&tab.id)) {
            return self;
        }

        let tabs = self
            .tabs
            .into_iter()
            .map(|tab| {
                if hidden.contains(&tab.id) {
                    tab.touched()
                } else {
                    tab
                }
            })
            .collect();
        Self { tabs, ..self }
    }

    fn shown_tabs(&self) -> HashSet<TabId> {
        self.workspaces
            .iter()
            .flat_map(|workspace| {
                workspace
                    .active_tab_id
                    .into_iter()
                    .chain(workspace.pane_tree.iter().flat_map(|tree| tree.leaves()))
            })
            .collect()
    }

    /// Closes the tabs [`Self::idle_tabs`] finds, except `spared`, in one
    /// transition, with each one's place before any of them closed.
    pub fn with_idle_tabs_closed(
        &self,
        now: DateTime<Utc>,
        after: TabArchiveAfter,
        spared: &HashSet<TabId>,
    ) -> WisResult<(Self, Vec<Archived>)> {
        let mut state = self.clone();
        let mut archived = Vec::new();
        for id in self.idle_tabs(now, after) {
            if spared.contains(&id) {
                continue;
            }

            let tab = self.tab(id)?.clone();
            let position = self
                .workspace(tab.workspace_id)?
                .tab_order
                .iter()
                .position(|&ordered| ordered == id);
            state = state.with_tab_closed(id)?;
            archived.push(Archived { tab, position });
        }

        Ok((state, archived))
    }

    /// Web tabs unused for `after`, by the clock. Pinned tabs, tabs in
    /// folders, the tabs a workspace shows and private workspaces' tabs
    /// are unaffected.
    pub fn idle_tabs(&self, now: DateTime<Utc>, after: TabArchiveAfter) -> Vec<TabId> {
        let Some(idle_for) = after.idle_for() else {
            return Vec::new();
        };
        self.tabs
            .iter()
            .filter(|tab| {
                let Ok(workspace) = self.workspace(tab.workspace_id) else {
                    return false;
                };
                let shown = workspace.active_tab_id == Some(tab.id)
                    || workspace
                        .pane_tree
                        .as_ref()
                        .is_some_and(|tree| tree.contains(tab.id));
                !workspace.ephemeral
                    && !shown
                    && !tab.pinned
                    && tab.folder_id.is_none()
                    && matches!(tab.url.scheme(), "http" | "https")
                    && now - tab.last_active_at >= idle_for
            })
            .map(|tab| tab.id)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, Duration, Utc};

    use super::*;
    use crate::ids::{TabFolderId, TabId};
    use crate::pane::PaneNode;
    use crate::state::BrowserState;
    use crate::workspace::AccentColor;

    fn idle_since(state: BrowserState, id: TabId, at: DateTime<Utc>) -> BrowserState {
        let tabs = state
            .tabs
            .iter()
            .map(|tab| {
                if tab.id == id {
                    crate::tab::Tab {
                        last_active_at: at,
                        ..tab.clone()
                    }
                } else {
                    tab.clone()
                }
            })
            .collect();
        BrowserState { tabs, ..state }
    }

    #[test]
    fn web_tabs_idle_past_the_limit_close_unless_pinned_or_in_a_folder() {
        let now = Utc::now();
        let long_ago = now - Duration::hours(13);
        let state = BrowserState::new_with_defaults();
        let ws = state.active_workspace_id;
        let (state, idle) = state.with_new_tab(ws, "https://idle.test/").unwrap();
        let (state, recent) = state.with_new_tab(ws, "https://recent.test/").unwrap();
        let (state, pinned) = state.with_new_tab(ws, "https://pinned.test/").unwrap();
        let (state, _) = state.with_tab_pinned(pinned.id, true).unwrap();
        let (state, foldered) = state.with_new_tab(ws, "https://folder.test/").unwrap();
        let (state, wis_page) = state.with_new_tab(ws, "wis://settings").unwrap();
        let (state, active) = state.with_new_tab(ws, "https://active.test/").unwrap();

        let mut state = [idle.id, pinned.id, foldered.id, wis_page.id, active.id]
            .into_iter()
            .fold(state, |state, id| idle_since(state, id, long_ago));
        state = idle_since(state, recent.id, now - Duration::hours(1));
        state.tabs = state
            .tabs
            .into_iter()
            .map(|tab| crate::tab::Tab {
                folder_id: (tab.id == foldered.id).then(TabFolderId::new),
                ..tab
            })
            .collect();

        assert_eq!(
            state.idle_tabs(now, TabArchiveAfter::TwelveHours),
            [idle.id]
        );
        assert!(state.idle_tabs(now, TabArchiveAfter::OneDay).is_empty());
        assert!(state.idle_tabs(now, TabArchiveAfter::Off).is_empty());
    }

    #[test]
    fn private_workspaces_and_split_panes_keep_their_tabs() {
        let now = Utc::now();
        let long_ago = now - Duration::days(40);
        let state = BrowserState::new_with_defaults();
        let ws = state.active_workspace_id;
        let (state, private) =
            state.with_new_private_workspace("Private".into(), AccentColor::Primary);
        let (state, secret) = state
            .with_new_tab(private.id, "https://secret.test/")
            .unwrap();
        let (state, left) = state.with_new_tab(ws, "https://left.test/").unwrap();
        let (state, right) = state.with_new_tab(ws, "https://right.test/").unwrap();
        let (mut state, _) = state.with_new_tab(ws, "https://active.test/").unwrap();
        state.workspaces = state
            .workspaces
            .into_iter()
            .map(|w| crate::workspace::Workspace {
                pane_tree: (w.id == ws).then(|| PaneNode::leaf(left.id)),
                ..w
            })
            .collect();
        let state = [secret.id, left.id, right.id]
            .into_iter()
            .fold(state, |state, id| idle_since(state, id, long_ago));

        assert_eq!(
            state.idle_tabs(now, TabArchiveAfter::ThirtyDays),
            [right.id]
        );
    }

    #[test]
    fn a_tab_stops_being_shown_counts_as_its_last_use() {
        let state = BrowserState::new_with_defaults();
        let ws = state.active_workspace_id;
        let (state, reading) = state.with_new_tab(ws, "https://reading.test/").unwrap();
        let long_ago = Utc::now() - Duration::days(3);
        let before = idle_since(state, reading.id, long_ago);
        let (after, opened) = before.with_new_tab(ws, "https://opened.test/").unwrap();

        let after = after.with_hidden_tabs_touched(&before);
        assert!(after.tab(reading.id).unwrap().last_active_at > long_ago);
        assert_eq!(
            after.tab(opened.id).unwrap().last_active_at,
            opened.last_active_at
        );
    }

    #[test]
    fn a_split_pane_closing_counts_as_its_last_use_but_a_closed_tab_stays_closed() {
        let state = BrowserState::new_with_defaults();
        let ws = state.active_workspace_id;
        let (state, left) = state.with_new_tab(ws, "https://left.test/").unwrap();
        let (mut state, _) = state.with_new_tab(ws, "https://active.test/").unwrap();
        state.workspaces = state
            .workspaces
            .into_iter()
            .map(|w| crate::workspace::Workspace {
                pane_tree: (w.id == ws).then(|| PaneNode::leaf(left.id)),
                ..w
            })
            .collect();
        let long_ago = Utc::now() - Duration::days(3);
        let before = idle_since(state, left.id, long_ago);

        let unsplit = before.with_panes_unsplit(ws).unwrap();
        assert!(
            unsplit
                .with_hidden_tabs_touched(&before)
                .tab(left.id)
                .unwrap()
                .last_active_at
                > long_ago
        );
        let closed = before.with_tab_closed(left.id).unwrap();
        assert!(closed
            .with_hidden_tabs_touched(&before)
            .tab(left.id)
            .is_err());
    }

    #[test]
    fn idle_tabs_close_in_one_go_with_their_places_except_the_spared() {
        let now = Utc::now();
        let long_ago = now - Duration::days(2);
        let state = BrowserState::new_with_defaults();
        let ws = state.active_workspace_id;
        let (state, first) = state.with_new_tab(ws, "https://first.test/").unwrap();
        let (state, playing) = state.with_new_tab(ws, "https://playing.test/").unwrap();
        let (state, third) = state.with_new_tab(ws, "https://third.test/").unwrap();
        let (state, _) = state.with_new_tab(ws, "https://active.test/").unwrap();
        let state = [first.id, playing.id, third.id]
            .into_iter()
            .fold(state, |state, id| idle_since(state, id, long_ago));

        let spared = std::collections::HashSet::from([playing.id]);
        let (state, archived) = state
            .with_idle_tabs_closed(now, TabArchiveAfter::OneDay, &spared)
            .unwrap();

        let places: Vec<(TabId, Option<usize>)> =
            archived.iter().map(|a| (a.tab.id, a.position)).collect();
        assert_eq!(places, [(first.id, Some(0)), (third.id, Some(2))]);
        assert!(state.tab(first.id).is_err());
        assert!(state.tab(playing.id).is_ok());
    }

    #[test]
    fn each_choice_is_its_own_idle_time() {
        assert_eq!(TabArchiveAfter::default(), TabArchiveAfter::Off);
        assert_eq!(TabArchiveAfter::Off.idle_for(), None);
        assert_eq!(
            TabArchiveAfter::TwelveHours.idle_for(),
            Some(Duration::hours(12))
        );
        assert_eq!(
            TabArchiveAfter::SevenDays.idle_for(),
            Some(Duration::days(7))
        );
    }
}

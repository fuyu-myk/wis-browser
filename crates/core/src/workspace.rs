//! Workspace: a named group of tabs with its own accent colour,
//! essentials, and privacy preset.

use crate::background::Background;
use crate::ids::{TabFolderId, TabId, WorkspaceId};
use crate::pane::{PaneBranch, PaneNode, SplitDir};
use crate::privacy::PrivacyMode;
use crate::proxy::UpstreamProxy;
use crate::tab_folder::TabFolder;
use serde::{Deserialize, Serialize};

/// Accent colours available for a workspace,
/// to colour the workspace pill, active-tab indicator, etc.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccentColor {
    #[default]
    Primary,
    Mint,
    Rose,
    Sun,
    Lila,
    Gray,
    Ink,
    Azur,
}

/// One of the four "essentials" pinned shortcuts shown above the tab list.
/// Slot indices are 0..=3.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EssentialPin {
    pub slot: u8,
    pub label: String,
    pub url: String,
    pub icon: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Workspace {
    pub id: WorkspaceId,
    pub name: String,
    pub accent: AccentColor,
    pub icon: Option<String>,
    pub tab_order: Vec<TabId>,
    pub pinned_tab_ids: Vec<TabId>,
    /// Live-tab folders in this workspace. Membership is on the tab
    /// ([`Tab::folder_id`](crate::tab::Tab::folder_id)).
    /// Serde-defaults to empty so existing state loads unchanged.
    #[serde(default)]
    pub tab_folders: Vec<TabFolder>,
    /// Folder that new tabs are filed into (Cmd+T, bookmarks, etc.). `None`
    /// means new tabs follow the active tab. Session-only (not persisted).
    #[serde(default)]
    pub selected_folder_id: Option<TabFolderId>,
    pub essentials: Vec<EssentialPin>,
    pub active_tab_id: Option<TabId>,
    pub privacy_mode: PrivacyMode,
    /// In-window split layout. `None` shows a single tab (`active_tab_id`);
    /// `Some(tree)` tiles the leaf tabs side-by-side with `active_tab_id` as
    /// the focused pane. Serde-defaults to `None` so existing state loads
    /// unchanged. See [`crate::pane`].
    #[serde(default)]
    pub pane_tree: Option<PaneNode>,
    /// When true this is a private / incognito workspace: its tabs use a
    /// non-persistent WebView data store and the workspace (and its tabs,
    /// essentials, history) is never written to disk. Defaults to `false` so
    /// existing persisted state deserialises unchanged.
    #[serde(default)]
    pub ephemeral: bool,
    /// Upstream proxy this workspace routes traffic through (e.g. Tor via a
    /// local SOCKS5). `None` = direct connection. Serde-defaults to `None`.
    #[serde(default)]
    pub upstream_proxy: Option<UpstreamProxy>,
    /// Custom page background (gradient or imported wallpaper). `None` uses
    /// the theme default. Serde-defaults to `None`.
    #[serde(default)]
    pub background: Option<Background>,
    /// User-picked flat `#rrggbb` colour for the workspace dot, overriding
    /// the `accent` preset's swatch. Cleared when a preset is picked.
    #[serde(default)]
    pub custom_accent: Option<String>,
}

impl Workspace {
    pub fn new(name: String, accent: AccentColor) -> Self {
        Self::new_inner(name, accent, false)
    }

    /// Construct a private / incognito workspace (see [`Workspace::ephemeral`]).
    pub fn new_private(name: String, accent: AccentColor) -> Self {
        Self::new_inner(name, accent, true)
    }

    fn new_inner(name: String, accent: AccentColor, ephemeral: bool) -> Self {
        Self {
            id: WorkspaceId::new(),
            name,
            accent,
            icon: None,
            tab_order: Vec::new(),
            pinned_tab_ids: Vec::new(),
            tab_folders: Vec::new(),
            selected_folder_id: None,
            essentials: Vec::new(),
            active_tab_id: None,
            privacy_mode: PrivacyMode::default(),
            pane_tree: None,
            ephemeral,
            upstream_proxy: None,
            background: None,
            custom_accent: None,
        }
    }

    pub fn with_name(&self, name: String) -> Self {
        Self {
            name,
            ..self.clone()
        }
    }

    pub fn with_accent(&self, accent: AccentColor) -> Self {
        Self {
            accent,
            custom_accent: None,
            ..self.clone()
        }
    }

    pub fn with_custom_accent(&self, color: Option<String>) -> Self {
        Self {
            custom_accent: color,
            ..self.clone()
        }
    }

    /// Append a tab to the workspace. If the workspace had no active tab,
    /// the new tab becomes active.
    pub fn with_tab_appended(&self, tab_id: TabId) -> Self {
        let mut next = self.clone();

        if !next.tab_order.contains(&tab_id) {
            next.tab_order.push(tab_id);
        }

        if next.active_tab_id.is_none() {
            next.active_tab_id = Some(tab_id);
        }

        next
    }

    /// Remove a tab. If it was active, the previous tab in `tab_order` becomes
    /// active (or `None` if no tabs remain).
    pub fn with_tab_removed(&self, tab_id: TabId) -> Self {
        let mut next = self.clone();
        let removed_index = next.tab_order.iter().position(|t| *t == tab_id);

        next.tab_order.retain(|t| *t != tab_id);
        next.pinned_tab_ids.retain(|t| *t != tab_id);

        if next.active_tab_id == Some(tab_id) {
            next.active_tab_id = match removed_index {
                Some(idx) if idx > 0 => next.tab_order.get(idx - 1).copied(),
                Some(_) => next.tab_order.first().copied(),
                None => next.tab_order.last().copied(),
            };
        }

        next
    }

    /// Replace the tab ordering wholesale. The caller is responsible for
    /// ensuring the new order contains exactly the same tab IDs as before
    /// (validated at the [`crate::state::BrowserState`] level).
    pub fn with_tab_order(&self, order: Vec<TabId>) -> Self {
        Self {
            tab_order: order,
            ..self.clone()
        }
    }

    pub fn with_active_tab(&self, tab_id: TabId) -> Self {
        Self {
            active_tab_id: Some(tab_id),
            ..self.clone()
        }
    }

    pub fn with_pinned(&self, tab_id: TabId, pinned: bool) -> Self {
        let mut next = self.clone();
        if pinned {
            if !next.pinned_tab_ids.contains(&tab_id) {
                next.pinned_tab_ids.push(tab_id);
            }
        } else {
            next.pinned_tab_ids.retain(|t| *t != tab_id);
        }

        next
    }

    pub fn with_privacy_mode(&self, mode: PrivacyMode) -> Self {
        Self {
            privacy_mode: mode,
            ..self.clone()
        }
    }

    // -- tab folders --------------------------------------------------------

    /// Whether a folder with `id` exists in this workspace.
    pub fn has_folder(&self, id: TabFolderId) -> bool {
        self.tab_folders.iter().any(|f| f.id == id)
    }

    /// Next free folder position (one past the current maximum).
    pub fn next_folder_position(&self) -> u32 {
        self.tab_folders
            .iter()
            .map(|f| f.position)
            .max()
            .map_or(0, |m| m + 1)
    }

    pub fn with_tab_folder_appended(&self, folder: TabFolder) -> Self {
        let mut next = self.clone();
        next.tab_folders.push(folder);
        next
    }

    fn map_folder(&self, id: TabFolderId, f: impl Fn(&TabFolder) -> TabFolder) -> Self {
        Self {
            tab_folders: self
                .tab_folders
                .iter()
                .map(|fold| if fold.id == id { f(fold) } else { fold.clone() })
                .collect(),
            ..self.clone()
        }
    }

    pub fn with_folder_renamed(&self, id: TabFolderId, name: String) -> Self {
        self.map_folder(id, |f| f.with_name(name.clone()))
    }

    pub fn with_folder_recolored(&self, id: TabFolderId, accent: AccentColor) -> Self {
        self.map_folder(id, |f| f.with_accent(accent))
    }

    pub fn with_folder_collapsed(&self, id: TabFolderId, collapsed: bool) -> Self {
        self.map_folder(id, |f| f.with_collapsed(collapsed))
    }

    pub fn with_folder_removed(&self, id: TabFolderId) -> Self {
        Self {
            tab_folders: self
                .tab_folders
                .iter()
                .filter(|f| f.id != id)
                .cloned()
                .collect(),
            selected_folder_id: self.selected_folder_id.filter(|s| *s != id),
            ..self.clone()
        }
    }

    pub fn with_selected_folder(&self, id: Option<TabFolderId>) -> Self {
        Self {
            selected_folder_id: id,
            ..self.clone()
        }
    }

    /// Reassign folder positions to match `order` (folders not listed keep their
    /// relative order after the listed ones).
    pub fn with_tab_folders_reordered(&self, order: &[TabFolderId]) -> Self {
        let tab_folders = self
            .tab_folders
            .iter()
            .map(|f| match order.iter().position(|id| *id == f.id) {
                Some(idx) => f.with_position(idx as u32),
                None => f.with_position(order.len() as u32 + f.position),
            })
            .collect();

        Self {
            tab_folders,
            ..self.clone()
        }
    }

    pub fn with_ephemeral(&self, ephemeral: bool) -> Self {
        Self {
            ephemeral,
            ..self.clone()
        }
    }

    pub fn with_upstream_proxy(&self, upstream_proxy: Option<UpstreamProxy>) -> Self {
        Self {
            upstream_proxy,
            ..self.clone()
        }
    }

    pub fn with_background(&self, background: Option<Background>) -> Self {
        Self {
            background,
            ..self.clone()
        }
    }

    // -- panes --------------------------------------------------------------

    /// Tabs currently displayed: the pane leaves when split, otherwise just the
    /// active tab (or nothing when the workspace is empty).
    pub fn displayed_tab_ids(&self) -> Vec<TabId> {
        match &self.pane_tree {
            Some(tree) => tree.leaves(),
            None => self.active_tab_id.into_iter().collect(),
        }
    }

    /// Replace the whole pane layout.
    pub fn with_pane_tree(&self, pane_tree: Option<PaneNode>) -> Self {
        Self {
            pane_tree,
            ..self.clone()
        }
    }

    /// Focus `tab` as the active pane. When split and `tab` isn't already a
    /// displayed leaf, it takes over the currently-focused pane (the page in
    /// the focused viewport is swapped for `tab`). When not split this is a
    /// plain active-tab change.
    pub fn with_focused_tab(&self, tab: TabId) -> Self {
        match &self.pane_tree {
            Some(tree) if !tree.contains(tab) => {
                let focused = self.active_tab_id.unwrap_or_else(|| tree.first_leaf());
                Self {
                    pane_tree: Some(tree.with_leaf_replaced(focused, tab)),
                    active_tab_id: Some(tab),
                    ..self.clone()
                }
            }
            _ => Self {
                active_tab_id: Some(tab),
                ..self.clone()
            },
        }
    }

    /// Split the focused pane along `dir`, placing `new_tab` in the new pane and
    /// focusing it. `place_new_first` puts `new_tab` in the left/top slot.
    /// Starts a fresh tree from the active tab when not yet split. `new_tab`
    /// must already be present in `tab_order` (the caller adds it).
    pub fn with_active_pane_split(
        &self,
        dir: SplitDir,
        new_tab: TabId,
        place_new_first: bool,
    ) -> Self {
        let Some(focused) = self.active_tab_id else {
            return self.clone();
        };
        let base = self
            .pane_tree
            .clone()
            .unwrap_or_else(|| PaneNode::leaf(focused));

        Self {
            pane_tree: Some(base.with_split(focused, dir, new_tab, place_new_first)),
            active_tab_id: Some(new_tab),
            ..self.clone()
        }
    }

    /// Swap the tab shown by the `target` pane leaf for `replacement`, focusing
    /// it. If `replacement` is already shown elsewhere it's removed from there
    /// first (so it appears once). No-op if `target == replacement`.
    pub fn with_pane_leaf_swapped(&self, target: TabId, replacement: TabId) -> Self {
        if target == replacement {
            return self.clone();
        }
        let Some(tree) = &self.pane_tree else {
            return Self {
                active_tab_id: Some(replacement),
                ..self.clone()
            };
        };
        let base = if tree.contains(replacement) {
            tree.with_closed_leaf(replacement)
                .unwrap_or_else(|| PaneNode::leaf(target))
        } else {
            tree.clone()
        };

        Self {
            pane_tree: Some(base.with_leaf_replaced(target, replacement)),
            active_tab_id: Some(replacement),
            ..self.clone()
        }
    }

    /// Split the `target` pane along `dir`, placing the already-open `moved` tab
    /// in the new pane (and focusing it). If `moved` is currently shown in
    /// another pane it is first removed from there so it appears exactly once.
    /// `place_new_first` puts `moved` in the left/top slot. No-op if
    /// `target == moved`.
    pub fn with_pane_split_with(
        &self,
        target: TabId,
        dir: SplitDir,
        place_new_first: bool,
        moved: TabId,
    ) -> Self {
        if target == moved {
            return self.clone();
        }

        let anchor = self.active_tab_id.unwrap_or(target);
        let mut base = self
            .pane_tree
            .clone()
            .unwrap_or_else(|| PaneNode::leaf(anchor));
        if base.contains(moved) {
            base = base
                .with_closed_leaf(moved)
                .unwrap_or_else(|| PaneNode::leaf(target));
        }
        if !base.contains(target) {
            base = PaneNode::leaf(target);
        }

        Self {
            pane_tree: Some(base.with_split(target, dir, moved, place_new_first)),
            active_tab_id: Some(moved),
            ..self.clone()
        }
    }

    /// Reconcile the pane layout after `removed` has left `tab_order`. Collapses
    /// the split when only one leaf remains and keeps `active_tab_id` pointing at
    /// a surviving leaf.
    pub fn with_pane_after_removed(&self, removed: TabId) -> Self {
        let Some(tree) = &self.pane_tree else {
            return self.clone();
        };
        if !tree.contains(removed) {
            return self.clone();
        }

        match tree.with_closed_leaf(removed) {
            None => Self {
                pane_tree: None,
                ..self.clone()
            },
            Some(t) if t.is_leaf() => Self {
                pane_tree: None,
                active_tab_id: Some(t.first_leaf()),
                ..self.clone()
            },
            Some(t) => {
                let active = match self.active_tab_id {
                    Some(a) if t.contains(a) => Some(a),
                    _ => Some(t.first_leaf()),
                };
                Self {
                    pane_tree: Some(t),
                    active_tab_id: active,
                    ..self.clone()
                }
            }
        }
    }

    /// Set the ratio (permille) of the split reached by `path`.
    pub fn with_pane_ratio(&self, path: &[PaneBranch], ratio: u16) -> Self {
        match &self.pane_tree {
            Some(tree) => Self {
                pane_tree: Some(tree.with_ratio_at(path, ratio)),
                ..self.clone()
            },
            None => self.clone(),
        }
    }
}

//! Chrome design: geometry and material knobs that restyle the chrome's
//! *form* the way [`crate::theme`] restyles its colour. Corner rounding, tab
//! and folder shape, liquid-glass scoping/intensity, density, the active-tab
//! indicator, close-button behaviour, and sidebar width.
//!
//! Stored on [`crate::UserSettings`] as a single field.
//! Every variant defaults to today's look, so existing settings deserialise unchanged.
//! The frontend mirrors each field onto the `<body>` element and CSS does the rest;
//! the one numeric field is clamped at the IPC boundary via [`ChromeDesign::sanitized`].

use serde::{Deserialize, Serialize};

pub const SIDEBAR_WIDTH_MIN: u32 = 180;
pub const SIDEBAR_WIDTH_MAX: u32 = 360;
pub const SIDEBAR_WIDTH_DEFAULT: u32 = 240;

/// Corner rounding of chrome surfaces. Drives a global radius scale; pills and
/// circles stay round (squaring a pill tab is [`TabShape`]'s job, not this).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CornerStyle {
    #[default]
    Round,
    Soft,
    Sharp,
}

/// Shape of tab chips.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TabShape {
    Pill,
    #[default]
    Rounded,
    Square,
}

/// How a tab folder's label renders.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FolderStyle {
    /// Accent-filled name chip (today's look).
    #[default]
    Pill,
    /// Accent outline, transparent fill.
    Outline,
    /// Plain text, no chip.
    Text,
}

/// Backdrop-blur strength for glass surfaces.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GlassIntensity {
    Light,
    #[default]
    Medium,
    Heavy,
}

/// Style of the active-tab indicator.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TabIndicator {
    /// Edge bar.
    #[default]
    Bar,
    Underline,
    /// Filled accent background on the active tab.
    Filled,
    Dot,
}

/// Overall chrome spacing.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Density {
    Comfortable,
    #[default]
    Cozy,
    Compact,
}

/// When the per-tab close button is shown.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloseButton {
    Always,
    /// Visible on hover/active.
    #[default]
    Hover,
    Hidden,
}

/// Which glass surfaces blur when the master `glass_enabled` toggle is on.
/// All true reproduces today's all-or-nothing behaviour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct GlassScopes {
    pub chrome: bool,
    pub menus: bool,
    pub tabs: bool,
}

impl Default for GlassScopes {
    fn default() -> Self {
        Self {
            chrome: true,
            menus: true,
            tabs: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChromeDesign {
    #[serde(default)]
    pub corners: CornerStyle,
    #[serde(default)]
    pub tab_shape: TabShape,
    #[serde(default)]
    pub folder_style: FolderStyle,
    #[serde(default)]
    pub glass_scopes: GlassScopes,
    #[serde(default)]
    pub glass_intensity: GlassIntensity,
    #[serde(default)]
    pub tab_indicator: TabIndicator,
    #[serde(default)]
    pub density: Density,
    #[serde(default)]
    pub close_button: CloseButton,
    #[serde(default = "default_sidebar_width")]
    pub sidebar_width: u32,
}

fn default_sidebar_width() -> u32 {
    SIDEBAR_WIDTH_DEFAULT
}

impl Default for ChromeDesign {
    fn default() -> Self {
        Self {
            corners: CornerStyle::default(),
            tab_shape: TabShape::default(),
            folder_style: FolderStyle::default(),
            glass_scopes: GlassScopes::default(),
            glass_intensity: GlassIntensity::default(),
            tab_indicator: TabIndicator::default(),
            density: Density::default(),
            close_button: CloseButton::default(),
            sidebar_width: SIDEBAR_WIDTH_DEFAULT,
        }
    }
}

impl ChromeDesign {
    /// Clamp boundary-supplied values into safe ranges. Pure; returns a copy.
    pub fn sanitized(&self) -> Self {
        Self {
            sidebar_width: self
                .sidebar_width
                .clamp(SIDEBAR_WIDTH_MIN, SIDEBAR_WIDTH_MAX),
            ..*self
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_todays_look() {
        let d = ChromeDesign::default();
        assert_eq!(d.corners, CornerStyle::Round);
        assert_eq!(d.tab_shape, TabShape::Rounded);
        assert_eq!(d.folder_style, FolderStyle::Pill);
        assert_eq!(d.glass_intensity, GlassIntensity::Medium);
        assert_eq!(d.tab_indicator, TabIndicator::Bar);
        assert_eq!(d.density, Density::Cozy);
        assert_eq!(d.close_button, CloseButton::Hover);
        assert_eq!(d.sidebar_width, SIDEBAR_WIDTH_DEFAULT);
        assert_eq!(
            d.glass_scopes,
            GlassScopes {
                chrome: true,
                menus: true,
                tabs: true,
            }
        );
    }

    #[test]
    fn sanitized_clamps_sidebar_width() {
        let too_small = ChromeDesign {
            sidebar_width: 10,
            ..ChromeDesign::default()
        };
        assert_eq!(too_small.sanitized().sidebar_width, SIDEBAR_WIDTH_MIN);

        let too_large = ChromeDesign {
            sidebar_width: 9_999,
            ..ChromeDesign::default()
        };
        assert_eq!(too_large.sanitized().sidebar_width, SIDEBAR_WIDTH_MAX);

        let ok = ChromeDesign {
            sidebar_width: 260,
            ..ChromeDesign::default()
        };
        assert_eq!(ok.sanitized().sidebar_width, 260);
    }

    #[test]
    fn missing_fields_deserialise_to_defaults() {
        // An older settings blob carries no design at all; an empty object
        // (or a partial one) must fill in every field.
        let partial: ChromeDesign = serde_json::from_str(r#"{"corners":"sharp"}"#).unwrap();
        assert_eq!(partial.corners, CornerStyle::Sharp);
        assert_eq!(partial.tab_shape, TabShape::Rounded);
        assert_eq!(partial.sidebar_width, SIDEBAR_WIDTH_DEFAULT);
    }

    #[test]
    fn round_trips_through_json() {
        let d = ChromeDesign {
            corners: CornerStyle::Sharp,
            tab_shape: TabShape::Square,
            folder_style: FolderStyle::Text,
            glass_scopes: GlassScopes {
                chrome: true,
                menus: false,
                tabs: true,
            },
            glass_intensity: GlassIntensity::Heavy,
            tab_indicator: TabIndicator::Dot,
            density: Density::Compact,
            close_button: CloseButton::Hidden,
            sidebar_width: 300,
        };
        let json = serde_json::to_string(&d).unwrap();
        let back: ChromeDesign = serde_json::from_str(&json).unwrap();
        assert_eq!(back, d);
    }
}

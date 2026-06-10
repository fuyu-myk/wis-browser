//! Per-workspace backgrounds: a custom gradient or an imported wallpaper.
//!
//! Pure data plus validation. Rendering happens in the chrome (the gradient
//! becomes a CSS `linear-gradient`; an image is served via the `wis-asset`
//! protocol from the content-addressed wallpaper store). All inputs cross an
//! IPC boundary, so every variant validates before entering state.

use serde::{Deserialize, Serialize};

use crate::error::{WisError, WisResult};

/// Gradients keep a small, UI-manageable number of stops.
pub const MAX_GRADIENT_STOPS: usize = 8;
/// Image dim overlay cap (percent): full blackout is never useful.
pub const MAX_IMAGE_DIM: u8 = 80;
/// Image backdrop blur cap (px).
pub const MAX_IMAGE_BLUR: u8 = 40;

/// A workspace's background.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Background {
    Gradient(GradientSpec),
    Image(ImageBackground),
}

/// A linear gradient: CSS angle + ordered colour stops.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GradientSpec {
    /// Degrees, `0..=180` (CSS `linear-gradient` angle).
    pub angle: u16,
    /// `2..=MAX_GRADIENT_STOPS` stops, positions ascending.
    pub stops: Vec<GradientStop>,
    /// Whole-gradient opacity percent, `0..=100` (scales every stop's alpha
    /// at render time). Serde-defaults to fully opaque.
    #[serde(default = "default_gradient_opacity")]
    pub opacity: u8,
}

fn default_gradient_opacity() -> u8 {
    100
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GradientStop {
    /// `#rrggbb` or `#rrggbbaa` hex colour.
    pub color: String,
    /// Percent along the gradient line, `0..=100`.
    pub position: u8,
}

/// How an imported wallpaper fills the page.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImageFit {
    #[default]
    Cover,
    Contain,
    Tile,
}

/// An imported wallpaper, referenced by the sha256 of its bytes (the shell's
/// wallpaper store is content-addressed, like favicons).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageBackground {
    /// Lowercase sha256 hex of the asset file.
    pub asset: String,
    #[serde(default)]
    pub fit: ImageFit,
    /// Darkening overlay percent, `0..=MAX_IMAGE_DIM`.
    #[serde(default)]
    pub dim: u8,
    /// Backdrop blur px, `0..=MAX_IMAGE_BLUR`.
    #[serde(default)]
    pub blur: u8,
}

/// `#rrggbb` or `#rrggbbaa` (lowercase or uppercase hex digits).
pub fn is_valid_hex_color(s: &str) -> bool {
    (s.len() == 7 || s.len() == 9)
        && s.starts_with('#')
        && s[1..].chars().all(|c| c.is_ascii_hexdigit())
}

impl Background {
    /// Boundary validation for IPC-supplied values.
    pub fn validate(&self) -> WisResult<()> {
        match self {
            Background::Gradient(g) => g.validate(),
            Background::Image(img) => img.validate(),
        }
    }
}

impl GradientSpec {
    pub fn validate(&self) -> WisResult<()> {
        if self.angle > 180 {
            return Err(invalid("gradient angle must be 0..=180 degrees"));
        }
        if self.opacity > 100 {
            return Err(invalid("gradient opacity is 0..=100 percent"));
        }
        if self.stops.len() < 2 || self.stops.len() > MAX_GRADIENT_STOPS {
            return Err(invalid("gradient needs 2 to 8 stops"));
        }

        for stop in &self.stops {
            if !is_valid_hex_color(&stop.color) {
                return Err(invalid("gradient colours must be #rrggbb hex"));
            }
            if stop.position > 100 {
                return Err(invalid("gradient stop positions are 0..=100 percent"));
            }
        }
        if self.stops.windows(2).any(|w| w[0].position > w[1].position) {
            return Err(invalid("gradient stop positions must be ascending"));
        }

        Ok(())
    }
}

impl ImageBackground {
    pub fn validate(&self) -> WisResult<()> {
        let valid_asset =
            self.asset.len() == 64 && self.asset.chars().all(|c| c.is_ascii_hexdigit());
        if !valid_asset {
            return Err(invalid("image asset must be a sha256 hex digest"));
        }
        if self.dim > MAX_IMAGE_DIM {
            return Err(invalid("image dim is capped at 80 percent"));
        }
        if self.blur > MAX_IMAGE_BLUR {
            return Err(invalid("image blur is capped at 40 px"));
        }

        Ok(())
    }
}

fn invalid(message: &str) -> WisError {
    WisError::InvalidInput {
        message: message.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gradient() -> Background {
        Background::Gradient(GradientSpec {
            angle: 160,
            opacity: 100,
            stops: vec![
                GradientStop {
                    color: "#efe7ff".into(),
                    position: 0,
                },
                GradientStop {
                    color: "#f7f6f3".into(),
                    position: 100,
                },
            ],
        })
    }

    #[test]
    fn valid_gradient_passes() {
        gradient().validate().unwrap();
    }

    type GradientMutation = Box<dyn Fn(&mut GradientSpec)>;

    #[test]
    fn gradient_rejects_bad_inputs() {
        let cases: Vec<GradientMutation> = vec![
            Box::new(|g| g.angle = 181),
            Box::new(|g| g.opacity = 101),
            Box::new(|g| g.stops.truncate(1)),
            Box::new(|g| g.stops[0].color = "efe7ff".into()),
            Box::new(|g| g.stops[0].color = "#efe7fg".into()),
            Box::new(|g| g.stops[1].position = 101),
            Box::new(|g| {
                g.stops[0].position = 80;
                g.stops[1].position = 20;
            }),
        ];
        for mutate in cases {
            let Background::Gradient(mut g) = gradient() else {
                unreachable!()
            };
            mutate(&mut g);
            assert!(Background::Gradient(g).validate().is_err());
        }
    }

    #[test]
    fn image_validation_bounds() {
        let mut img = ImageBackground {
            asset: "a".repeat(64),
            fit: ImageFit::Cover,
            dim: 30,
            blur: 10,
        };
        Background::Image(img.clone()).validate().unwrap();

        img.asset = "zz".repeat(32);
        assert!(Background::Image(img.clone()).validate().is_err());
        img.asset = "a".repeat(64);
        img.dim = 81;
        assert!(Background::Image(img.clone()).validate().is_err());
        img.dim = 0;
        img.blur = 41;
        assert!(Background::Image(img).validate().is_err());
    }

    #[test]
    fn background_serde_round_trips() {
        let json = serde_json::to_string(&gradient()).unwrap();
        let back: Background = serde_json::from_str(&json).unwrap();
        assert_eq!(back, gradient());
        assert!(json.contains("\"gradient\""), "externally tagged: {json}");
    }

    #[test]
    fn hex_color_check() {
        assert!(is_valid_hex_color("#A1b2C3"));
        assert!(is_valid_hex_color("#a1b2c3ff"));
        assert!(!is_valid_hex_color("#a1b2c3f"));
        assert!(!is_valid_hex_color("#a1b2c"));
        assert!(!is_valid_hex_color("a1b2c3f"));
        assert!(!is_valid_hex_color("#a1b2cg"));
    }
}

//! The declarations that make a stand-in selection look like the real one.
//!
//! The page menu repaints a selection the engine has stopped drawing, but two
//! things make that harder than reading a colour:
//!
//! 1. **No engine paints the raw system colour**. `NSColor` hands back an
//!    opaque colour, and WebKit makes it translucent itself. Blink and
//!    WebKitGTK do the same with their own numbers.
//! 2. **Selected text usually keeps its own colour**. WebKit overrides the
//!    foreground only in a dark document, and WebKitGTK never does. Blink is
//!    the exception and does override, on both.
//!
//! Which branch applies is decided by the **page**, and not by the system or
//! the app. WebKit reads the document's used `color-scheme`, and CSS
//! `light-dark()` resolves through the same thing.
//!
//! macOS and Linux are reproduced exactly. On Windows the CSS system keywords
//! stand in, with Blink's alpha applied over them.

/// CSS for the `::highlight()` rule, as two declaration blocks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectionCss {
    /// Written first, using only long-supported syntax. An engine that cannot
    /// parse [`Self::preferred`] drops it and keeps this.
    pub fallback: String,
    /// What the engine should use. Written second so it wins where it parses.
    pub preferred: String,
}

/// The declarations a `::highlight()` rule should carry on this platform.
#[cfg(not(target_os = "linux"))]
pub fn selection_css() -> SelectionCss {
    imp::selection_css().unwrap_or_else(keyword_css)
}

/// The declarations a `::highlight()` rule should carry in `webview`. Runs
/// on the GTK main thread.
#[cfg(target_os = "linux")]
pub fn selection_css(webview: &webkit2gtk::WebView) -> SelectionCss {
    accent_css(imp::accent(webview))
}

/// The CSS system keywords, for a platform that cannot be asked. `Highlight`
/// already tracks the document's colour scheme, so no branch is needed.
fn keyword_css() -> SelectionCss {
    SelectionCss {
        fallback: "background-color:Highlight".to_string(),
        preferred: "background-color:Highlight".to_string(),
    }
}

/// `rgba(r, g, b, a)` from 8-bit channels and an 8-bit alpha.
fn css_rgba(rgb: (u8, u8, u8), alpha: u8) -> String {
    let (red, green, blue) = rgb;
    if alpha == 255 {
        return format!("rgb({red}, {green}, {blue})");
    }
    let fraction = (f64::from(alpha) / 255.0 * 1000.0).round() / 1000.0;

    format!("rgba({red}, {green}, {blue}, {fraction})")
}

/// WebKit's `blendWithWhite`, which is how a light-appearance selection
/// becomes translucent.
///
/// Ported from `WebCore/platform/graphics/ColorBlending.cpp`, including its
/// truncating integer division.
fn blend_with_white(rgb: (u8, u8, u8)) -> ((u8, u8, u8), u8) {
    const START_ALPHA: i32 = 153;
    const END_ALPHA: i32 = 204;
    const ALPHA_STEP: i32 = 17;

    let (red, green, blue) = rgb;
    let mut result = (rgb, END_ALPHA as u8);

    let mut alpha = START_ALPHA;
    while alpha <= END_ALPHA {
        let white_blend = 255 - alpha;
        let component = |channel: u8| (i32::from(channel) - white_blend) * 255 / alpha;
        let (red, green, blue) = (component(red), component(green), component(blue));
        let clamp = |value: i32| value.clamp(0, 255) as u8;

        result = ((clamp(red), clamp(green), clamp(blue)), alpha as u8);
        if red >= 0 && green >= 0 && blue >= 0 {
            break;
        }
        alpha += ALPHA_STEP;
    }

    result
}

/// The dark branch is a flat alpha on the untouched channels, with no blend.
const DARK_ALPHA: u8 = 204;

/// WebKitGTK's accent when the theme names none.
#[cfg(any(target_os = "linux", test))]
const ADWAITA_ACCENT: (u8, u8, u8) = (52, 132, 228);

/// `RenderThemeAdwaita::platformActiveSelectionBackgroundColor`.
#[cfg(any(target_os = "linux", test))]
const ADWAITA_SELECTION_ALPHA: f32 = 0.3;

/// The selection WebKitGTK paints for a GTK accent, given as red, green, blue
/// and alpha from 0 to 1. It has no dark branch and sets no foreground.
///
/// Rounds as WebKit's `Color(GdkRGBA)` and `colorWithAlphaMultipliedBy` do.
#[cfg(any(target_os = "linux", test))]
fn accent_css(accent: Option<(f64, f64, f64, f64)>) -> SelectionCss {
    let byte = |value: f64| (value.clamp(0.0, 1.0) as f32 * 255.0).round() as u8;
    let (rgb, alpha) = match accent {
        Some((red, green, blue, alpha)) => ((byte(red), byte(green), byte(blue)), byte(alpha)),
        None => (ADWAITA_ACCENT, 255),
    };
    let alpha = (ADWAITA_SELECTION_ALPHA * (f32::from(alpha) / 255.0) * 255.0).round() as u8;
    let background = format!("background-color:{}", css_rgba(rgb, alpha));

    SelectionCss {
        fallback: background.clone(),
        preferred: background,
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use super::{blend_with_white, css_rgba, SelectionCss, DARK_ALPHA};
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};

    pub(super) fn selection_css() -> Option<SelectionCss> {
        // Both branches are resolved here rather than left to the page.
        let light = unsafe { resolve("NSAppearanceNameAqua")? };
        let dark = unsafe { resolve("NSAppearanceNameDarkAqua")? };

        let (light_rgb, light_alpha) = blend_with_white(light.background);
        let light_background = css_rgba(light_rgb, light_alpha);
        let dark_background = css_rgba(dark.background, DARK_ALPHA);
        let dark_foreground = css_rgba(dark.foreground, 255);

        Some(SelectionCss {
            fallback: format!("background-color:{light_background}"),
            preferred: format!(
                "background-color:light-dark({light_background}, {dark_background});\
                 color:light-dark(currentColor, {dark_foreground})"
            ),
        })
    }

    struct Resolved {
        background: (u8, u8, u8),
        foreground: (u8, u8, u8),
    }

    /// Read both selection colours with `name` as the drawing appearance.
    unsafe fn resolve(name: &str) -> Option<Resolved> {
        let appearance = appearance_named(name)?;

        let background = std::cell::Cell::new(None);
        let foreground = std::cell::Cell::new(None);
        {
            let block = block2::StackBlock::new(|| {
                background.set(read(msg_send![
                    class!(NSColor),
                    selectedTextBackgroundColor
                ]));
                foreground.set(read(msg_send![class!(NSColor), selectedTextColor]));
            });
            let _: () = msg_send![appearance, performAsCurrentDrawingAppearance: &*block];
        }

        Some(Resolved {
            background: background.get()?,
            foreground: foreground.get()?,
        })
    }

    unsafe fn appearance_named(name: &str) -> Option<*mut AnyObject> {
        let c = std::ffi::CString::new(name).ok()?;
        let ns: *mut AnyObject = msg_send![
            class!(NSString),
            stringWithUTF8String: c.as_ptr()
        ];
        if ns.is_null() {
            return None;
        }

        let appearance: *mut AnyObject = msg_send![class!(NSAppearance), appearanceNamed: ns];

        (!appearance.is_null()).then_some(appearance)
    }

    /// A named colour is in a catalog space with no components, so it is
    /// converted to sRGB before being read.
    unsafe fn read(color: *mut AnyObject) -> Option<(u8, u8, u8)> {
        if color.is_null() {
            return None;
        }

        let space: *mut AnyObject = msg_send![class!(NSColorSpace), sRGBColorSpace];
        let converted: *mut AnyObject = msg_send![color, colorUsingColorSpace: space];
        if converted.is_null() {
            return None;
        }

        let red: f64 = msg_send![converted, redComponent];
        let green: f64 = msg_send![converted, greenComponent];
        let blue: f64 = msg_send![converted, blueComponent];
        let channel = |value: f64| (value.clamp(0.0, 1.0) * 255.0).round() as u8;

        Some((channel(red), channel(green), channel(blue)))
    }
}

/// Blink paints its own colour rather than the system one, except in forced
/// colours, where the keyword resolves through `GetSysColor` anyway.
#[cfg(windows)]
mod imp {
    use super::SelectionCss;

    /// `LayoutTheme::ActiveSelectionBackgroundColor` blends to this.
    const BLINK_ALPHA_PERCENT: u8 = 80;

    pub(super) fn selection_css() -> Option<SelectionCss> {
        Some(SelectionCss {
            fallback: "background-color:Highlight;color:HighlightText".to_string(),
            preferred: format!(
                "background-color:color-mix(in srgb, Highlight {BLINK_ALPHA_PERCENT}%, transparent);\
                 color:HighlightText"
            ),
        })
    }
}

/// WebKitGTK paints the accent its web view's GTK theme names. `Highlight`
/// cannot stand in, since WebKitGTK pins it to its own blue so that a page
/// cannot read the theme.
#[cfg(target_os = "linux")]
mod imp {
    use gtk::prelude::{StyleContextExt, WidgetExt};

    /// The names `PageClientImpl::accentColor` looks up, in its order.
    const ACCENT_NAMES: [&str; 4] = [
        "accent_bg_color",
        "accent_color",
        "accentColor",
        "theme_selected_bg_color",
    ];

    pub(super) fn accent(webview: &webkit2gtk::WebView) -> Option<(f64, f64, f64, f64)> {
        let context = webview.style_context();

        ACCENT_NAMES
            .iter()
            .find_map(|name| context.lookup_color(name))
            .map(|color| (color.red(), color.green(), color.blue(), color.alpha()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_opaque_channel_set_is_written_without_alpha() {
        assert_eq!(css_rgba((0, 128, 255), 255), "rgb(0, 128, 255)");
    }

    #[test]
    fn alpha_is_written_as_a_fraction() {
        assert_eq!(css_rgba((1, 2, 3), 204), "rgba(1, 2, 3, 0.8)");
        assert_eq!(css_rgba((1, 2, 3), 153), "rgba(1, 2, 3, 0.6)");
    }

    /// The measured default macOS purple accent. Composited over white this
    /// has to come back to the colour it started as.
    #[test]
    fn the_white_blend_matches_webkits_own_arithmetic() {
        assert_eq!(blend_with_white((223, 197, 224)), ((201, 158, 203), 153));
    }

    #[test]
    fn a_blended_colour_composites_back_to_the_original() {
        let original = (223, 197, 224);
        let ((red, green, blue), alpha) = blend_with_white(original);
        let over_white = |channel: u8| {
            let a = f64::from(alpha) / 255.0;
            (f64::from(channel) * a + 255.0 * (1.0 - a)).round() as u8
        };
        assert_eq!(
            (over_white(red), over_white(green), over_white(blue)),
            original
        );
    }

    /// A colour dark enough that 60% cannot express it steps up until one can.
    #[test]
    fn a_dark_colour_walks_the_alpha_up() {
        let (_, alpha) = blend_with_white((25, 103, 210));
        assert!(alpha > 153, "expected a step up, got {alpha}");
        assert!(
            alpha <= 204,
            "expected the walk to stop at 80%, got {alpha}"
        );
    }

    #[test]
    fn a_gtk_accent_is_painted_at_thirty_percent_with_no_foreground() {
        let accent = (53.0 / 255.0, 132.0 / 255.0, 228.0 / 255.0, 1.0);
        let css = accent_css(Some(accent));
        assert_eq!(css.fallback, "background-color:rgba(53, 132, 228, 0.302)");
        assert_eq!(css.preferred, css.fallback);
    }

    #[test]
    fn a_theme_with_no_accent_gets_webkitgtks_own_blue() {
        assert_eq!(
            accent_css(None).preferred,
            "background-color:rgba(52, 132, 228, 0.302)"
        );
    }

    /// The accent's own alpha is rounded to a byte before the 30% applies.
    #[test]
    fn a_translucent_accent_multiplies_its_own_alpha() {
        assert_eq!(
            accent_css(Some((1.0, 0.0, 0.0, 0.5))).preferred,
            "background-color:rgba(255, 0, 0, 0.149)"
        );
    }

    #[test]
    fn accent_channels_outside_the_range_are_clamped() {
        assert_eq!(
            accent_css(Some((1.5, -0.2, 0.5, 2.0))).preferred,
            "background-color:rgba(255, 0, 128, 0.302)"
        );
    }

    #[test]
    fn the_fallback_never_sets_a_foreground_it_cannot_justify() {
        // Overriding the glyph colour is Blink's behaviour, not a default.
        assert!(!keyword_css().fallback.contains("color:HighlightText"));
    }
}

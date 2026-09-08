//! Save and copy the image under the page context menu.
//!
//! Both commands re-fetch the image through the workspace's loopback proxy,
//! so DoH and the tracker rules apply and the request looks like every other
//! one the workspace makes. The page hands over a URL and never bytes.
//!
//! `is_public_web_url` is the gate. Both commands hand their result to the
//! user, one to a file and one to the clipboard. It screens the scheme and
//! any literal address.
//!
//! The fetch is capped at [`MAX_IMAGE_BYTES`] while it streams.

use std::time::Duration;

use tauri::{AppHandle, State};
use url::Url;
use wis_core::{WisError, WisResult, WorkspaceId};

use crate::favicons::{sniff_format, Format};
use crate::state::AppState;

/// Generous next to the favicon's 5 s: this is a user-initiated fetch of a
/// full-size image, and the workspace may be routed through Tor.
const IMAGE_TIMEOUT: Duration = Duration::from_secs(30);
/// Room for a full-resolution photograph, without letting a hostile page
/// make the fetch eat memory.
const MAX_IMAGE_BYTES: usize = 20 * 1024 * 1024;
/// Well past any photograph on the web, and far below what a decoder would
/// have to reserve gigabytes for.
const MAX_PIXELS: u64 = 64_000_000;
/// Enough for the CDN hops a real image URL takes, few enough to bound the
/// number of requests one menu click makes.
const MAX_REDIRECTS: usize = 5;
/// Used when the URL's last path segment is empty or all separators.
const FALLBACK_STEM: &str = "image";

fn invalid(message: impl Into<String>) -> WisError {
    WisError::InvalidInput {
        message: message.into(),
    }
}

/// Fetch `src` through `workspace_id`'s proxy and identify its format by
/// magic bytes. The declared content type is not trusted.
async fn load_image(
    state: &AppState,
    workspace_id: WorkspaceId,
    src: &str,
) -> WisResult<(Vec<u8>, Format)> {
    let url = Url::parse(src).map_err(|_| WisError::InvalidUrl {
        url: src.to_string(),
    })?;
    if !wis_network::is_public_web_url(&url) {
        return Err(invalid("that image is not on the public web"));
    }

    let proxy_port = state.ensure_proxy_for(workspace_id).await?;
    let proxy = reqwest::Proxy::all(format!("http://127.0.0.1:{proxy_port}"))
        .map_err(|err| invalid(format!("proxy: {err}")))?;
    let redirects = reqwest::redirect::Policy::custom(|attempt| {
        if attempt.previous().len() >= MAX_REDIRECTS {
            return attempt.error("too many redirects");
        }
        if !wis_network::is_public_web_url(attempt.url()) {
            return attempt.error("that image redirects off the public web");
        }

        attempt.follow()
    });
    let client = reqwest::Client::builder()
        .timeout(IMAGE_TIMEOUT)
        .redirect(redirects)
        .user_agent(wis_core::user_agent::compatibility_user_agent(
            wis_core::UaPlatform::host(),
        ))
        .proxy(proxy)
        .build()
        .map_err(|err| invalid(format!("build client: {err}")))?;

    let mut resp = client
        .get(url.as_str())
        .send()
        .await
        .map_err(|err| invalid(format!("fetching the image failed: {err}")))?;
    if !resp.status().is_success() {
        return Err(invalid(format!(
            "the server answered {} for that image",
            resp.status()
        )));
    }

    let mut bytes: Vec<u8> = Vec::new();
    while let Some(chunk) = resp
        .chunk()
        .await
        .map_err(|err| invalid(format!("reading the image failed: {err}")))?
    {
        if bytes.len() + chunk.len() > MAX_IMAGE_BYTES {
            return Err(invalid("that image is too large to save"));
        }
        bytes.extend_from_slice(&chunk);
    }
    if bytes.is_empty() {
        return Err(invalid("the server returned an empty image"));
    }

    let format =
        sniff_format(&bytes).ok_or_else(|| invalid("that is not an image Wis can read"))?;

    Ok((bytes, format))
}

/// A save-panel filename for `src`. The stem comes from the URL's last path
/// segment. The extension comes from the sniffed format, never from the URL.
fn suggested_name(src: &str, format: Format) -> String {
    let segment = Url::parse(src)
        .ok()
        .and_then(|url| {
            url.path_segments()?
                .rfind(|segment| !segment.is_empty())
                .map(str::to_string)
        })
        .unwrap_or_default();
    // Drop the URL's own extension.
    let stem = segment
        .rsplit_once('.')
        .map_or(segment.as_str(), |(head, _)| head);
    let stem = if stem.trim().is_empty() {
        FALLBACK_STEM
    } else {
        stem
    };

    let ext = format.extension();
    let name = wis_core::sanitize_filename(&format!("{stem}.{ext}"));
    if name.ends_with(&format!(".{ext}")) {
        name
    } else {
        format!("{FALLBACK_STEM}.{ext}")
    }
}

/// Fetch the image at `src` and write it wherever the save panel points.
/// `None` when the panel is cancelled.
#[tauri::command]
pub async fn save_image_as(
    app: AppHandle,
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
    src: String,
) -> WisResult<Option<String>> {
    let (bytes, format) = load_image(&state, workspace_id, &src).await?;

    // The panel must not hold a runtime worker.
    let name = suggested_name(&src, format);
    let panel_app = app.clone();
    let chosen =
        tokio::task::spawn_blocking(move || crate::panels::run_save_panel(&panel_app, &name))
            .await
            .map_err(|err| invalid(format!("save panel: {err}")))??;
    let Some(path) = chosen else {
        return Ok(None);
    };
    tokio::fs::write(&path, &bytes)
        .await
        .map_err(|err| invalid(format!("writing {}: {err}", path.display())))?;

    Ok(Some(path.to_string_lossy().into_owned()))
}

/// Fetch the image at `src` and put it on the clipboard as an image.
///
/// SVG is refused everywhere: it is markup, and neither clipboard writer
/// rasterises it. Off macOS only PNG is accepted, because the arboard path
/// decodes PNG and nothing else.
#[tauri::command]
pub async fn copy_image(
    app: AppHandle,
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
    src: String,
) -> WisResult<()> {
    let (bytes, format) = load_image(&state, workspace_id, &src).await?;
    if let Some(refusal) = uncopyable(&bytes, format) {
        return Err(invalid(refusal));
    }

    if crate::screenshot::copy_image_bytes(&app, bytes).await {
        Ok(())
    } else {
        Err(invalid("copying the image to the clipboard failed"))
    }
}

/// Why these bytes cannot go on the clipboard here, or `None` when they can.
///
/// [`MAX_IMAGE_BYTES`] bounds what arrives on the wire; nothing bounds what
/// it expands to. The dimensions are read from the header and screened
/// before any decoder sees the bytes.
fn uncopyable(bytes: &[u8], format: Format) -> Option<&'static str> {
    if matches!(format, Format::Svg) {
        return Some("an SVG cannot be copied as an image, only as its address");
    }

    #[cfg(not(target_os = "macos"))]
    if !matches!(format, Format::Png) {
        return Some("only PNG images can be copied on this platform");
    }

    match image_dimensions(bytes, format) {
        None => Some("that image's size could not be read"),
        Some((width, height)) if u64::from(width) * u64::from(height) > MAX_PIXELS => {
            Some("that image is too large to copy")
        }
        Some(_) => None,
    }
}

/// Pixel dimensions from the header alone, without decoding. `None` when the
/// header is truncated or malformed.
fn image_dimensions(bytes: &[u8], format: Format) -> Option<(u32, u32)> {
    let le16 = |at: usize| -> Option<u32> {
        Some(u32::from(u16::from_le_bytes([
            *bytes.get(at)?,
            *bytes.get(at + 1)?,
        ])))
    };

    match format {
        // The signature is 8 bytes and IHDR must be the first chunk.
        Format::Png => Some((
            u32::from_be_bytes(bytes.get(16..20)?.try_into().ok()?),
            u32::from_be_bytes(bytes.get(20..24)?.try_into().ok()?),
        )),
        // Logical screen descriptor, little-endian, straight after "GIF89a".
        Format::Gif => Some((le16(6)?, le16(8)?)),
        // Width and height are one byte each, where 0 means 256.
        Format::Ico => {
            let side = |at: usize| -> Option<u32> {
                Some(match *bytes.get(at)? {
                    0 => 256,
                    value => u32::from(value),
                })
            };
            Some((side(6)?, side(7)?))
        }
        Format::Jpeg => jpeg_dimensions(bytes),
        Format::Webp => webp_dimensions(bytes, le16),
        Format::Svg => None,
    }
}

/// Walk the marker segments to the frame header, which is the only place a
/// JPEG states its size.
fn jpeg_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    let mut at = 2;
    loop {
        // Any number of fill bytes may precede a marker.
        while *bytes.get(at)? == 0xFF && *bytes.get(at + 1)? == 0xFF {
            at += 1;
        }
        if *bytes.get(at)? != 0xFF {
            return None;
        }

        let marker = *bytes.get(at + 1)?;
        if matches!(marker, 0x01 | 0xD0..=0xD9) {
            at += 2;
            continue;
        }

        let length = usize::from(u16::from_be_bytes([
            *bytes.get(at + 2)?,
            *bytes.get(at + 3)?,
        ]));
        if matches!(marker, 0xC0..=0xCF) && !matches!(marker, 0xC4 | 0xC8 | 0xCC) {
            let height = u16::from_be_bytes([*bytes.get(at + 5)?, *bytes.get(at + 6)?]);
            let width = u16::from_be_bytes([*bytes.get(at + 7)?, *bytes.get(at + 8)?]);
            return Some((u32::from(width), u32::from(height)));
        }
        if length < 2 {
            return None;
        }

        at += 2 + length;
    }
}

/// WebP states its size three different ways, one per sub-format.
fn webp_dimensions(bytes: &[u8], le16: impl Fn(usize) -> Option<u32>) -> Option<(u32, u32)> {
    match bytes.get(12..16)? {
        // Lossy: 14-bit dimensions after the start code.
        b"VP8 " => Some((le16(26)? & 0x3FFF, le16(28)? & 0x3FFF)),
        // Lossless: 14 bits each, packed across four bytes after the marker.
        b"VP8L" => {
            let packed = u32::from_le_bytes(bytes.get(21..25)?.try_into().ok()?);
            Some(((packed & 0x3FFF) + 1, ((packed >> 14) & 0x3FFF) + 1))
        }
        // Extended: 24-bit canvas size, stored one less than it is.
        b"VP8X" => {
            let at24 = |at: usize| -> Option<u32> {
                Some(
                    u32::from(*bytes.get(at)?)
                        | u32::from(*bytes.get(at + 1)?) << 8
                        | u32::from(*bytes.get(at + 2)?) << 16,
                )
            };
            Some((at24(24)? + 1, at24(27)? + 1))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_comes_from_the_last_path_segment() {
        assert_eq!(
            suggested_name("https://example.com/a/b/photo.jpg", Format::Jpeg),
            "photo.jpg"
        );
    }

    #[test]
    fn the_sniffed_format_wins_over_the_urls_claim() {
        assert_eq!(
            suggested_name("https://example.com/photo.jpg", Format::Png),
            "photo.png"
        );
    }

    #[test]
    fn a_query_string_is_not_part_of_the_name() {
        assert_eq!(
            suggested_name("https://example.com/photo.png?w=1200&sig=abc", Format::Png),
            "photo.png"
        );
    }

    #[test]
    fn a_pathless_url_falls_back() {
        assert_eq!(
            suggested_name("https://example.com/", Format::Webp),
            "image.webp"
        );
    }

    #[test]
    fn separators_in_the_segment_cannot_escape_the_chosen_directory() {
        let name = suggested_name("https://example.com/..%2F..%2Fetc/passwd", Format::Png);
        assert!(!name.contains('/'), "{name}");
        assert!(!name.contains('\\'), "{name}");
    }

    #[test]
    fn a_leading_dot_segment_does_not_produce_a_hidden_file() {
        let name = suggested_name("https://example.com/.hidden.png", Format::Png);
        assert!(!name.starts_with('.'), "{name}");
    }

    #[test]
    fn a_stem_the_sanitiser_eats_keeps_its_extension() {
        assert_eq!(
            suggested_name("https://example.com/.../", Format::Png),
            "image.png"
        );
    }

    /// A PNG header declaring `width` by `height`, which is all the guard
    /// reads. The pixel data a real file would carry is not needed.
    fn png_header(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        bytes.extend_from_slice(&width.to_be_bytes());
        bytes.extend_from_slice(&height.to_be_bytes());
        bytes
    }

    #[test]
    fn svg_is_never_copyable() {
        assert!(uncopyable(b"<svg/>", Format::Svg).is_some());
    }

    #[test]
    fn an_ordinary_png_is_copyable() {
        assert!(uncopyable(&png_header(1920, 1080), Format::Png).is_none());
    }

    /// The whole point of the guard: this is about 50 KB on the wire and
    /// would have a decoder reserve well over a gigabyte.
    #[test]
    fn a_decompression_bomb_is_refused() {
        assert!(uncopyable(&png_header(20_000, 20_000), Format::Png).is_some());
    }

    #[test]
    fn a_truncated_header_is_refused_rather_than_guessed() {
        assert!(uncopyable(b"\x89PNG\r\n\x1a\n", Format::Png).is_some());
    }

    #[test]
    fn png_dimensions_come_from_ihdr() {
        assert_eq!(
            image_dimensions(&png_header(800, 600), Format::Png),
            Some((800, 600))
        );
    }

    #[test]
    fn gif_dimensions_are_little_endian() {
        let mut bytes = b"GIF89a".to_vec();
        bytes.extend_from_slice(&300u16.to_le_bytes());
        bytes.extend_from_slice(&200u16.to_le_bytes());
        assert_eq!(image_dimensions(&bytes, Format::Gif), Some((300, 200)));
    }

    #[test]
    fn jpeg_dimensions_come_from_the_frame_header() {
        // SOI, an APP0 segment to skip, then SOF0 with 480 by 640.
        let mut bytes = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x04, 0x00, 0x00];
        bytes.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x11, 0x08]);
        bytes.extend_from_slice(&480u16.to_be_bytes());
        bytes.extend_from_slice(&640u16.to_be_bytes());
        assert_eq!(image_dimensions(&bytes, Format::Jpeg), Some((640, 480)));
    }

    #[test]
    fn a_jpeg_with_no_frame_header_is_refused() {
        assert_eq!(
            image_dimensions(
                &[0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x04, 0x00, 0x00],
                Format::Jpeg
            ),
            None
        );
    }

    #[test]
    fn lossy_webp_dimensions_are_fourteen_bit() {
        let mut bytes = b"RIFF\0\0\0\0WEBPVP8 ".to_vec();
        // Chunk size, frame tag and start code, then the dimensions.
        bytes.extend_from_slice(&[0; 10]);
        bytes.extend_from_slice(&640u16.to_le_bytes());
        bytes.extend_from_slice(&480u16.to_le_bytes());
        assert_eq!(image_dimensions(&bytes, Format::Webp), Some((640, 480)));
    }

    #[test]
    fn extended_webp_canvas_is_stored_one_less() {
        let mut bytes = b"RIFF\0\0\0\0WEBPVP8X".to_vec();
        bytes.extend_from_slice(&[0; 8]);
        bytes.extend_from_slice(&[0x3F, 0x00, 0x00]); // 63, so 64 wide
        bytes.extend_from_slice(&[0x1F, 0x00, 0x00]); // 31, so 32 high
        assert_eq!(image_dimensions(&bytes, Format::Webp), Some((64, 32)));
    }
}

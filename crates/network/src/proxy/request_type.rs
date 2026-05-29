//! Map an HTTP request to a [`crate::RequestType`] so the policy engine
//! sees the same `RequestType` taxonomy as the Adblock-Plus filter syntax.
//!
//! The honest source of truth is the `Sec-Fetch-Dest` request header (set
//! by every modern browser). When absent: e.g. opaque cross-origin or
//! preflight: we conservatively classify as `Other`.

use hyper::HeaderMap;

use crate::RequestType;

/// Classify a request by inspecting its `Sec-Fetch-Dest` header.
pub fn classify(headers: &HeaderMap) -> RequestType {
    let Some(value) = headers.get("sec-fetch-dest") else {
        return RequestType::Other;
    };
    let Ok(text) = value.to_str() else {
        return RequestType::Other;
    };

    match text {
        "document" => RequestType::Document,
        "frame" | "iframe" => RequestType::Subdocument,
        "script" | "serviceworker" | "sharedworker" | "worker" | "audioworklet"
        | "paintworklet" => RequestType::Script,
        "style" => RequestType::Stylesheet,
        "image" | "imageset" => RequestType::Image,
        "font" => RequestType::Font,
        "audio" | "video" | "track" => RequestType::Media,
        "empty" => RequestType::Xhr,
        _ => RequestType::Other,
    }
}

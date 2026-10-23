//! A page script's message body, as the Windows and Linux dispatchers see it.
//!
//! Scripts post through `window.webkit.messageHandlers`, which WebKitGTK has
//! natively and the WebView2 shim emulates. Requiring the text alone silently
//! dropped every message from a script that posts the object.

pub(crate) fn json_body(body: &serde_json::Value) -> Option<serde_json::Value> {
    match body {
        serde_json::Value::String(raw) => serde_json::from_str(raw).ok(),
        serde_json::Value::Object(_) => Some(body.clone()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn an_object_is_taken_as_it_came() {
        let body = json!({ "kind": "focus", "x": 1.0 });
        assert_eq!(json_body(&body), Some(body));
    }

    #[test]
    fn json_text_is_parsed() {
        let body = json!(r#"{"kind":"focus"}"#);
        assert_eq!(json_body(&body), Some(json!({ "kind": "focus" })));
    }

    #[test]
    fn anything_else_is_dropped() {
        for body in [json!(null), json!(3), json!([1]), json!("not json")] {
            assert_eq!(json_body(&body), None, "{body}");
        }
    }
}

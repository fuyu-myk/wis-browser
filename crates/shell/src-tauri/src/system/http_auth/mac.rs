//! WebKit's http authentication: wry hands every challenge to one hook,
//! which names the tab by the WKWebView's identifier.

use tauri::AppHandle;

use super::{same_host, Answer, Challenge, Space};

pub type Reply = wry::AuthReply;

impl Answer for wry::AuthReply {
    fn sign_in(self, username: &str, password: &str) {
        wry::AuthReply::sign_in(self, username, password);
    }

    fn cancel(self) {
        wry::AuthReply::cancel(self);
    }
}

pub fn install(app: AppHandle) {
    wry::set_auth_challenge_hook(Box::new(move |challenge, reply| {
        // SAFETY: wry calls the hook on the main thread with a live WKWebView.
        let Some(label) = (unsafe { crate::platform::native_webview::label(challenge.webview) })
        else {
            return reply.cancel();
        };
        // SAFETY: as above.
        let page = unsafe { crate::platform::native_webview::url(challenge.webview) };
        let on_page = same_host(page.as_deref(), &challenge.host);
        let space = Space {
            label,
            host: challenge.host,
            port: challenge.port,
            protocol: challenge.protocol,
            realm: challenge.realm,
        };
        let challenge = Challenge {
            space,
            on_page,
            failed: challenge.previous_failures > 0,
        };
        super::offer(&app, challenge, reply);
    }));
}

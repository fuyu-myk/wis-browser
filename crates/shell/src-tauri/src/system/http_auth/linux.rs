//! WebKitGTK's http authentication: each webview holds its `authenticate`
//! requests until the card answers. WebKitGTK cancels a waiting request
//! itself when the page loads on, which withdraws its card.

use tauri::{AppHandle, Manager};
use webkit2gtk::glib::translate::ToGlibPtr;
use webkit2gtk::{
    AuthenticationRequest, AuthenticationRequestExt, AuthenticationScheme, Credential,
    CredentialPersistence, WebResourceExt, WebView, WebViewExt, WebsiteDataManagerExt,
};

use super::{same_host, Answer, Challenge, Space};

pub struct Reply {
    request: AuthenticationRequest,
    answered: bool,
}

impl Reply {
    /// Signs in with `credential`, or without one, which shows the server's
    /// refusal. Only the first answer counts.
    fn answer(&mut self, credential: Option<&Credential>) {
        if std::mem::replace(&mut self.answered, true) {
            return;
        }
        let raw_request: *mut webkit2gtk::ffi::WebKitAuthenticationRequest =
            self.request.to_glib_none().0;
        let raw_credential: *const webkit2gtk::ffi::WebKitCredential =
            credential.map_or(std::ptr::null(), |credential| credential.to_glib_none().0);
        // SAFETY: on the GTK main thread; both pointers are live for the
        // call, WebKit copies the credential, and it ignores a request it
        // already settled.
        unsafe {
            webkit2gtk::ffi::webkit_authentication_request_authenticate(
                raw_request,
                raw_credential.cast_mut(),
            );
        }
    }
}

impl Answer for Reply {
    fn sign_in(mut self, username: &str, password: &str) {
        let credential = Credential::new(username, password, CredentialPersistence::ForSession);
        self.answer(Some(&credential));
    }

    fn cancel(self) {
        drop(self);
    }
}

impl Drop for Reply {
    fn drop(&mut self) {
        self.answer(None);
    }
}

/// Takes over a webview's sign-in challenges, on the main thread.
pub fn attach(webview: &tauri::Webview, _first_page: Option<&str>) {
    let app = webview.app_handle().clone();
    let label = webview.label().to_owned();
    let attached = webview.with_webview(move |w| {
        let web_view = w.inner();
        if let Some(manager) = web_view.website_data_manager() {
            manager.set_persistent_credential_storage_enabled(false);
        }
        web_view.connect_authenticate(move |view, request| challenged(&app, &label, view, request));
    });
    if let Err(err) = attached {
        log::warn!("sign-in prompts off for a webview: {err}");
    }
}

/// Whether `host` is the page's: the one shown, or the one loading, which
/// `uri` names only once it commits when the page itself opened it.
fn is_page_host(view: &WebView, host: &str) -> bool {
    let shown = view.uri();
    let loading = view.main_resource().and_then(|resource| resource.uri());
    [shown, loading]
        .iter()
        .flatten()
        .any(|url| same_host(Some(url.as_str()), host))
}

/// A challenge another page's resource raises is answered without
/// credentials; a scheme Wis does not prompt for keeps the engine's dialog
/// only on a tab page's own host.
fn challenged(
    app: &AppHandle,
    label: &str,
    view: &WebView,
    request: &AuthenticationRequest,
) -> bool {
    use AuthenticationScheme::{HttpBasic, HttpDigest, Negotiate, Ntlm};

    let scheme = request.scheme();
    let prompted = matches!(scheme, HttpBasic | HttpDigest);
    if request.is_for_proxy() || !(prompted || matches!(scheme, Ntlm | Negotiate)) {
        return false;
    }

    let host = request.host().map(|host| host.to_string());
    let on_page = host.as_deref().is_some_and(|host| is_page_host(view, host));
    if !prompted && on_page && super::prompts_for(label) {
        return false;
    }

    let reply = Reply {
        request: request.clone(),
        answered: false,
    };
    let Some(host) = host.filter(|_| prompted) else {
        Answer::cancel(reply);
        return true;
    };

    let space = Space {
        label: label.to_owned(),
        host,
        port: isize::try_from(request.port()).unwrap_or_default(),
        protocol: request
            .security_origin()
            .and_then(|origin| origin.protocol())
            .map(|protocol| protocol.to_string())
            .unwrap_or_default(),
        realm: request.realm().map(|realm| realm.to_string()),
    };
    let (app_for_cancel, withdrawn) = (app.clone(), space.clone());
    request.connect_cancelled(move |_| super::withdraw(&app_for_cancel, &withdrawn));

    let challenge = Challenge {
        space,
        on_page,
        failed: request.is_retry(),
    };
    super::offer(app, challenge, reply);
    true
}

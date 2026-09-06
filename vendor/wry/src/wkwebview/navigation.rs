use objc2::rc::Retained;
use objc2::{msg_send, DeclaredClass};
use objc2_foundation::{
  ns_string, NSError, NSHTTPURLResponse, NSObjectProtocol, NSString, NSURLAuthenticationChallenge,
  NSURLAuthenticationMethodServerTrust, NSURLCredential, NSURLErrorFailingURLPeerTrustErrorKey,
  NSURLSessionAuthChallengeDisposition,
};
use objc2_web_kit::{
  WKNavigation, WKNavigationAction, WKNavigationActionPolicy, WKNavigationResponse,
  WKNavigationResponsePolicy,
};

#[cfg(target_os = "ios")]
use crate::wkwebview::ios::WKWebView::WKWebView;
#[cfg(target_os = "macos")]
use objc2_web_kit::WKWebView;

use crate::PageLoadEvent;

use super::class::wry_navigation_delegate::WryNavigationDelegate;

pub(crate) fn did_commit_navigation(
  this: &WryNavigationDelegate,
  webview: &WKWebView,
  _navigation: &WKNavigation,
) {
  unsafe {
    // Call on_load_handler
    if let Some(on_page_load) = &this.ivars().on_page_load_handler {
      on_page_load(PageLoadEvent::Started);
    }

    // Inject scripts
    let mut pending_scripts = this.ivars().pending_scripts.lock().unwrap();
    if let Some(scripts) = &*pending_scripts {
      for script in scripts {
        webview.evaluateJavaScript_completionHandler(&NSString::from_str(script), None);
      }
      *pending_scripts = None;
    }
  }
}

pub(crate) fn did_finish_navigation(
  this: &WryNavigationDelegate,
  _webview: &WKWebView,
  _navigation: &WKNavigation,
) {
  if let Some(on_page_load) = &this.ivars().on_page_load_handler {
    on_page_load(PageLoadEvent::Finished);
  }
}

// WIS patch: a provisional navigation failure (DNS, connection refused,
// timeout, TLS) produces no page-load event upstream, so the tab spins until
// the shell's progress poller times out. Stash the NSError's code + domain in
// the thread-local side channel and re-use the Finished event to deliver it.
// See WIS-PATCH.md.
pub(crate) fn did_fail_provisional_navigation(
  this: &WryNavigationDelegate,
  _webview: &WKWebView,
  error: &NSError,
) {
  super::set_last_page_load_error(
    error.code() as i64,
    error.domain().to_string(),
    failing_leaf_certificate_der(error),
  );
  if let Some(on_page_load) = &this.ivars().on_page_load_handler {
    on_page_load(PageLoadEvent::Finished);
  }
  super::take_last_page_load_error();
}

// WIS patch: Security/CoreFoundation calls used to read the leaf certificate
// out of a server-trust reference. `SecTrustGetCertificateAtIndex` returns a
// borrowed reference (no release), and the leaf at index 0 is always present
// without forcing a trust evaluation. Declared raw because neither framework
// has an objc2 binding in this dependency set.
#[link(name = "Security", kind = "framework")]
extern "C" {
  fn SecTrustGetCertificateAtIndex(
    trust: *const core::ffi::c_void,
    ix: isize,
  ) -> *const core::ffi::c_void;
  fn SecCertificateCopyData(cert: *const core::ffi::c_void) -> *const core::ffi::c_void;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
  fn CFDataGetLength(data: *const core::ffi::c_void) -> isize;
  fn CFDataGetBytePtr(data: *const core::ffi::c_void) -> *const u8;
  fn CFRelease(cf: *const core::ffi::c_void);
}

/// WIS patch: the DER bytes of `trust`'s leaf certificate, `None` when the
/// chain is missing or unreadable (callers fail closed on `None`). No trust
/// evaluation is triggered: only `SecTrustGetCertificateAtIndex(_, 0)` is
/// used, which the header guarantees returns the leaf without evaluating.
unsafe fn leaf_certificate_der(trust: *const core::ffi::c_void) -> Option<Vec<u8>> {
  if trust.is_null() {
    return None;
  }
  let cert = SecTrustGetCertificateAtIndex(trust, 0);
  if cert.is_null() {
    return None;
  }
  let data = SecCertificateCopyData(cert);
  if data.is_null() {
    return None;
  }
  let len = CFDataGetLength(data);
  let ptr = CFDataGetBytePtr(data);
  let der =
    (len > 0 && !ptr.is_null()).then(|| std::slice::from_raw_parts(ptr, len as usize).to_vec());
  CFRelease(data);
  der
}

/// WIS patch: the failing server's leaf certificate DER, read from the
/// provisional-failure `NSError`'s peer-trust userInfo
/// (`NSURLErrorFailingURLPeerTrustErrorKey` -> `SecTrustRef`). Bound to this
/// exact navigation, so a later challenge for the same host cannot substitute
/// a different certificate. `None` for non-TLS failures.
fn failing_leaf_certificate_der(error: &NSError) -> Option<Vec<u8>> {
  unsafe {
    let user_info = error.userInfo();
    // `objectForKey:` returns a borrowed (+0) SecTrustRef valid while
    // `user_info` is alive; read as an opaque pointer (no objc2 binding).
    let trust: *const core::ffi::c_void =
      msg_send![&*user_info, objectForKey: NSURLErrorFailingURLPeerTrustErrorKey];
    leaf_certificate_der(trust)
  }
}

// WIS patch: WebKit rejects an untrusted TLS certificate before
// `didFailProvisionalNavigation:` unless the server-trust challenge is
// answered with a credential. Every server-trust challenge is offered to the
// embedder-registered evaluator as `(host, leaf certificate DER)`; when it
// returns true, the challenge is answered with a credential built from that
// trust. Every other path keeps the system default handling. See WIS-PATCH.md.
pub(crate) fn did_receive_authentication_challenge(
  challenge: &NSURLAuthenticationChallenge,
  handler: &block2::Block<dyn Fn(NSURLSessionAuthChallengeDisposition, *mut NSURLCredential)>,
) {
  unsafe {
    let space = challenge.protectionSpace();
    if space
      .authenticationMethod()
      .isEqualToString(NSURLAuthenticationMethodServerTrust)
    {
      // `serverTrust` (`SecTrustRef`) has no objc2-foundation binding, so it
      // round-trips through `credentialForTrust:` as an opaque pointer.
      let trust: *mut core::ffi::c_void = msg_send![&*space, serverTrust];
      if !trust.is_null()
        && leaf_certificate_der(trust)
          .is_some_and(|der| super::server_trust_allowed(&space.host().to_string(), &der))
      {
        let credential: Option<Retained<NSURLCredential>> =
          msg_send![objc2::class!(NSURLCredential), credentialForTrust: trust];
        if let Some(credential) = credential {
          handler.call((
            NSURLSessionAuthChallengeDisposition::UseCredential,
            Retained::as_ptr(&credential) as *mut NSURLCredential,
          ));
          return;
        }
      }
    }

    handler.call((
      NSURLSessionAuthChallengeDisposition::PerformDefaultHandling,
      std::ptr::null_mut(),
    ));
  }
}

// Navigation handler
pub(crate) fn navigation_policy(
  this: &WryNavigationDelegate,
  _webview: &WKWebView,
  action: &WKNavigationAction,
  handler: &block2::Block<dyn Fn(WKNavigationActionPolicy)>,
) {
  unsafe {
    // <https://developer.apple.com/documentation/webkit/wknavigationaction/shouldperformdownload>
    // Available: macOS 11.3+, iOS 14.5+
    let can_download = action.respondsToSelector(objc2::sel!(shouldPerformDownload));
    let should_download: bool = if can_download {
      action.shouldPerformDownload()
    } else {
      false
    };
    let request = action.request();
    let url = request.URL().unwrap().absoluteString().unwrap();

    if should_download {
      let has_download_handler = this.ivars().has_download_handler;
      if has_download_handler {
        (*handler).call((WKNavigationActionPolicy::Download,));
      } else {
        (*handler).call((WKNavigationActionPolicy::Cancel,));
      }
    } else {
      let function = &this.ivars().navigation_policy_function;
      match function(url.to_string()) {
        true => (*handler).call((WKNavigationActionPolicy::Allow,)),
        false => (*handler).call((WKNavigationActionPolicy::Cancel,)),
      };
    }
  }
}

// WIS patch: the preferences variant of the policy decision. Runs the
// embedder hook (`set_webpage_preferences_hook`) on main-frame navigations,
// then the plain policy logic with a handler that passes those same
// preferences back to WebKit. See WIS-PATCH.md.
pub(crate) fn navigation_policy_with_preferences(
  this: &WryNavigationDelegate,
  webview: &WKWebView,
  action: &WKNavigationAction,
  preferences: &objc2::runtime::AnyObject,
  handler: &block2::Block<dyn Fn(WKNavigationActionPolicy, *mut objc2::runtime::AnyObject)>,
) {
  unsafe {
    let is_main_frame = action.targetFrame().is_some_and(|f| f.isMainFrame());
    if is_main_frame {
      let url = action
        .request()
        .URL()
        .and_then(|u| u.absoluteString())
        .map(|s| s.to_string())
        .unwrap_or_default();
      let prefs_ptr: *const objc2::runtime::AnyObject = preferences;
      super::apply_webpage_preferences(&url, prefs_ptr as *mut std::ffi::c_void);
    }

    let preferences = objc2::Message::retain(preferences);
    let outer = handler.copy();
    let inner = block2::RcBlock::new(move |policy: WKNavigationActionPolicy| {
      let ptr: *const objc2::runtime::AnyObject = Retained::as_ptr(&preferences);
      outer.call((policy, ptr as *mut objc2::runtime::AnyObject));
    });
    navigation_policy(this, webview, action, &inner);
  }
}

// Navigation handler
pub(crate) fn navigation_policy_response(
  this: &WryNavigationDelegate,
  _webview: &WKWebView,
  response: &WKNavigationResponse,
  handler: &block2::Block<dyn Fn(WKNavigationResponsePolicy)>,
) {
  unsafe {
    let can_show_mime_type = response.canShowMIMEType();
    // WIS patch: the embedder can force main-frame downloads (PDF, attachment
    // disposition) that WebKit would otherwise render inline. See WIS-PATCH.md.
    let forced = response.isForMainFrame() && {
      let resp = response.response();
      let mime_type = resp.MIMEType().map(|m| m.to_string());
      let disposition = resp
        .downcast::<NSHTTPURLResponse>()
        .ok()
        .and_then(|http| http.valueForHTTPHeaderField(ns_string!("Content-Disposition")))
        .map(|d| d.to_string());
      super::main_frame_download_forced(mime_type.as_deref(), disposition.as_deref())
    };

    if !can_show_mime_type || forced {
      let has_download_handler = this.ivars().has_download_handler;
      if has_download_handler {
        (*handler).call((WKNavigationResponsePolicy::Download,));
        return;
      }
    }

    (*handler).call((WKNavigationResponsePolicy::Allow,));
  }
}

pub(crate) fn web_content_process_did_terminate(
  this: &WryNavigationDelegate,
  _webview: &WKWebView,
) {
  if let Some(on_web_content_process_terminate) =
    &this.ivars().on_web_content_process_terminate_handler
  {
    on_web_content_process_terminate();
  }
}

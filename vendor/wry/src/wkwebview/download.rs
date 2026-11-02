#[cfg(target_os = "macos")]
use std::ptr::NonNull;
use std::{cell::RefCell, env::current_dir, ptr::null_mut};

#[cfg(target_os = "macos")]
use objc2::runtime::AnyObject;
use objc2::{DeclaredClass, Message, rc::Retained, runtime::ProtocolObject};
#[cfg(target_os = "macos")]
use objc2_foundation::NSURLRequest;
use objc2_foundation::{NSData, NSError, NSString, NSURL, NSURLResponse};
use objc2_web_kit::{WKDownload, WKNavigationAction, WKNavigationResponse};

#[cfg(target_os = "ios")]
use crate::wkwebview::ios::WKWebView::WKWebView;
#[cfg(target_os = "macos")]
use objc2_web_kit::WKWebView;

use super::class::{
  wry_download_delegate::WryDownloadDelegate, wry_navigation_delegate::WryNavigationDelegate,
};

// WIS patch: live downloads keyed by destination path, so the embedder can
// cancel one (`cancel_download_to`). Main-thread only, like every WKDownload
// delegate callback. Registered when a download is accepted; unregistered on
// finish/fail. See WIS-PATCH.md.
thread_local! {
  static ACTIVE_DOWNLOADS: RefCell<Vec<(String, Retained<WKDownload>)>> =
    const { RefCell::new(Vec::new()) };
}

fn unregister_download(download: &WKDownload) {
  let ptr: *const WKDownload = download;
  ACTIVE_DOWNLOADS.with(|list| {
    list
      .borrow_mut()
      .retain(|(_, d)| Retained::as_ptr(d) != ptr)
  });
}

/// WIS patch: cancel the in-flight download writing to `path`. Main thread
/// only. Returns whether a live download matched. Unregisters the entry
/// itself: WebKit suppresses the fail delegate for explicit cancels
/// (`DownloadManager::cancelDownload` passes `IgnoreDidFailCallback::Yes`),
/// so no delegate callback will do it. The entry is removed before the
/// ObjC call so the registry borrow never spans potential reentry.
pub fn cancel_download_to(path: &str) -> bool {
  let download = ACTIVE_DOWNLOADS.with(|list| {
    let mut list = list.borrow_mut();
    let pos = list.iter().position(|(p, _)| p == path)?;
    Some(list.remove(pos).1)
  });
  match download {
    Some(download) => {
      unsafe { download.cancel(None) };
      true
    }
    None => false,
  }
}

// Download action handler
pub(crate) fn navigation_download_action(
  this: &WryNavigationDelegate,
  _webview: &WKWebView,
  _action: &WKNavigationAction,
  download: &WKDownload,
) {
  unsafe {
    if let Some(delegate) = &this.ivars().download_delegate {
      let proto_delegate = ProtocolObject::from_ref(&**delegate);
      download.setDelegate(Some(proto_delegate));
    }
  }
}

// Download response handler
pub(crate) fn navigation_download_response(
  this: &WryNavigationDelegate,
  _webview: &WKWebView,
  _response: &WKNavigationResponse,
  download: &WKDownload,
) {
  unsafe {
    if let Some(delegate) = &this.ivars().download_delegate {
      let proto_delegate = ProtocolObject::from_ref(&**delegate);
      download.setDelegate(Some(proto_delegate));
    }
  }
}

/// WIS patch: download `url` in `webview`'s own session, through the download
/// delegate wry gave that web view, so the embedder's handlers pick the
/// destination and hear of the finish as for any other download. `started`
/// gets the URL the download will be reported under, before its destination
/// is asked for. Main thread only. False when the URL does not parse or
/// `webview` has no wry download delegate. See WIS-PATCH.md.
#[cfg(target_os = "macos")]
pub fn start_download(
  webview: &WKWebView,
  url: &str,
  started: impl FnOnce(String) + 'static,
) -> bool {
  // SAFETY: main thread, per this function's contract; a plain getter.
  let delegate = unsafe { webview.navigationDelegate() }.and_then(|navigation| {
    let navigation: &AnyObject = (*navigation).as_ref();
    navigation
      .downcast_ref::<WryNavigationDelegate>()?
      .ivars()
      .download_delegate
      .clone()
  });
  let Some(delegate) = delegate else {
    return false;
  };
  let Some(url) = NSURL::URLWithString(&NSString::from_str(url)) else {
    return false;
  };
  let request = NSURLRequest::requestWithURL(&url);
  let started = std::cell::Cell::new(Some(started));
  let handler = block2::RcBlock::new(move |download: NonNull<WKDownload>| {
    // SAFETY: WebKit calls this on the main thread with the new download,
    // before it asks the delegate for a destination.
    let reported = unsafe {
      let download = download.as_ref();
      download.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
      download
        .originalRequest()
        .and_then(|request| request.URL())
        .and_then(|url| url.absoluteString())
    };
    if let (Some(started), Some(reported)) = (started.take(), reported) {
      started(reported.to_string());
    }
  });
  // SAFETY: main thread; WebKit copies the block and the request.
  unsafe { webview.startDownloadUsingRequest_completionHandler(&request, &handler) };
  true
}

pub(crate) fn download_policy(
  this: &WryDownloadDelegate,
  download: &WKDownload,
  response: &NSURLResponse,
  suggested_filename: &NSString,
  completion_handler: &block2::Block<dyn Fn(*const NSURL)>,
) {
  unsafe {
    // WIS patch: stash the response's expected size so the started handler's
    // call stack can read it (`last_download_expected_bytes`).
    super::set_last_download_expected_bytes(response.expectedContentLength());
    let request = download.originalRequest().unwrap();
    let url = request.URL().unwrap().absoluteString().unwrap();
    let suggested_filename = suggested_filename.to_string();
    let mut download_destination =
      dirs::download_dir().unwrap_or_else(|| current_dir().unwrap_or_default());

    download_destination.push(&suggested_filename);

    let (suggested_filename, ext) = suggested_filename
      .split_once('.')
      .map(|(base, ext)| (base, format!(".{ext}")))
      .unwrap_or((&suggested_filename, "".to_string()));

    // WebView2 does not overwrite files but appends numbers
    let mut counter = 1;
    while download_destination.exists() {
      download_destination.set_file_name(format!("{suggested_filename} ({counter}){ext}"));
      counter += 1;
    }

    let started_fn = &this.ivars().started;
    if let Some(started_fn) = started_fn {
      let mut started_fn = started_fn.borrow_mut();
      match started_fn(url.to_string(), &mut download_destination) {
        true => {
          // WIS patch: track the accepted download for cancel-by-path.
          ACTIVE_DOWNLOADS.with(|list| {
            list.borrow_mut().push((
              download_destination.display().to_string(),
              download.retain(),
            ))
          });
          let path = NSString::from_str(&download_destination.display().to_string());
          let ns_url = NSURL::fileURLWithPath_isDirectory(&path, false);
          (*completion_handler).call((Retained::as_ptr(&ns_url),))
        }
        false => (*completion_handler).call((null_mut(),)),
      };
    } else {
      #[cfg(feature = "tracing")]
      tracing::warn!("WebView instance is dropped! This navigation handler shouldn't be called.");
      (*completion_handler).call((null_mut(),));
    }
  }
}

pub(crate) fn download_did_finish(this: &WryDownloadDelegate, download: &WKDownload) {
  unregister_download(download);
  unsafe {
    let original_request = download.originalRequest().unwrap();
    let url = original_request.URL().unwrap().absoluteString().unwrap();
    if let Some(completed_fn) = this.ivars().completed.clone() {
      completed_fn(url.to_string(), None, true);
    }
  }
}

pub(crate) fn download_did_fail(
  this: &WryDownloadDelegate,
  download: &WKDownload,
  error: &NSError,
  _resume_data: &NSData,
) {
  unregister_download(download);
  unsafe {
    #[cfg(debug_assertions)]
    {
      let description = error.localizedDescription().to_string();
      eprintln!("Download failed with error: {description}");
    }

    let original_request = download.originalRequest().unwrap();
    let url = original_request.URL().unwrap().absoluteString().unwrap();
    if let Some(completed_fn) = this.ivars().completed.clone() {
      completed_fn(url.to_string(), None, false);
    }
  }
}

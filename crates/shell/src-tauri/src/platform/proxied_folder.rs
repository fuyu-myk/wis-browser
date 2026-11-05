//! Windows WebView2 folders for workspaces behind Tor or SOCKS. A browser
//! process takes its flags at launch and keeps them while any webview in its
//! folder lives, so such a workspace gets a folder of its own whose process
//! always starts with the flags that keep pages behind the proxy. Its cookies
//! and storage stay apart from the workspace's direct ones.

use std::path::Path;

const SUFFIX: &str = "-proxied";

/// Loopback goes to the workspace proxy, which refuses it on these routes,
/// and WebRTC only over TCP through the proxy.
const BROWSER_ARGS: &str =
    "--proxy-bypass-list=<-loopback> --force-webrtc-ip-handling-policy=disable_non_proxied_udp";

/// The folder name for a workspace's webviews. Linux keeps one that only
/// names the WebKit context its cookies live in.
pub fn name(uuid: &str, proxied: bool) -> String {
    if proxied && cfg!(windows) {
        format!("{uuid}{SUFFIX}")
    } else {
        uuid.to_owned()
    }
}

/// The flags a folder's browser process adds to wry's own.
pub fn browser_args(dir: Option<&Path>) -> Option<&'static str> {
    let name = dir?.file_name()?.to_str()?;
    name.ends_with(SUFFIX).then_some(BROWSER_ARGS)
}

/// Gives every proxied folder's browser process its flags.
#[cfg(windows)]
pub fn install() {
    wry::set_browser_args_hook(Box::new(|dir| browser_args(dir).map(str::to_owned)));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_proxied_workspace_gets_its_own_folder_on_windows() {
        let uuid = "00000000-0000-4000-8000-000000000001";
        assert_eq!(name(uuid, false), uuid);
        let proxied = name(uuid, true);
        assert_eq!(proxied == format!("{uuid}-proxied"), cfg!(windows));
        assert_eq!(proxied == uuid, !cfg!(windows));
    }

    #[test]
    fn only_a_proxied_folder_starts_with_the_flags() {
        let base = Path::new("webviews");
        let args = browser_args(Some(
            &base.join("00000000-0000-4000-8000-000000000001-proxied"),
        ))
        .expect("a proxied folder gets flags");
        assert!(args.contains("--proxy-bypass-list=<-loopback>"));
        assert!(args.contains("--force-webrtc-ip-handling-policy=disable_non_proxied_udp"));
        assert_eq!(
            browser_args(Some(&base.join("00000000-0000-4000-8000-000000000001"))),
            None
        );
        assert_eq!(browser_args(None), None);
    }
}

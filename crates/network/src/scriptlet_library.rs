//! uBO's scriptlet library in the JSON form Ghostery's adblocker publishes,
//! converted to the engine's resources. uBO's redirect resources are left
//! out as neither WebKit's rule lists nor the CONNECT proxy can redirect.

use std::collections::HashSet;

use adblock::resources::{MimeType, PermissionMask, Resource, ResourceType};
use base64::Engine as _;
use serde::Deserialize;

use crate::error::{NetworkError, NetworkResult};

/// Granted to the lists uBO itself trusts; scriptlets marked as needing trust
/// run only from those.
pub const TRUSTED: PermissionMask = PermissionMask::from_bits(1);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Scriptlet {
    name: String,
    #[serde(default)]
    aliases: Vec<String>,
    body: String,
    #[serde(default)]
    dependencies: Vec<String>,
    #[serde(default)]
    requires_trust: bool,
}

#[derive(Deserialize)]
struct Library {
    scriptlets: Vec<Scriptlet>,
}

/// The engine would drop a scriptlet whose name is taken or whose helper is
/// missing without a word, so such a library is refused whole.
pub fn resources(json: &str) -> NetworkResult<Vec<Resource>> {
    let refuse = |why: String| NetworkError::FilterList(format!("scriptlet library: {why}"));
    let library: Library = serde_json::from_str(json).map_err(|err| refuse(err.to_string()))?;
    if library.scriptlets.is_empty() {
        return Err(refuse("no scriptlets".into()));
    }

    let mut names = HashSet::new();
    for scriptlet in &library.scriptlets {
        for name in std::iter::once(&scriptlet.name).chain(&scriptlet.aliases) {
            if !names.insert(name.as_str()) {
                return Err(refuse(format!("{name} is named twice")));
            }
        }
    }

    let helpers: HashSet<&str> = library.scriptlets.iter().map(|s| s.name.as_str()).collect();
    if let Some(missing) = library
        .scriptlets
        .iter()
        .flat_map(|s| &s.dependencies)
        .find(|dependency| !helpers.contains(dependency.as_str()))
    {
        return Err(refuse(format!("{missing} is missing")));
    }

    Ok(library.scriptlets.into_iter().map(resource).collect())
}

/// uBO registers `trusted-prevent-xhr` without the flag.
fn needs_trust(scriptlet: &Scriptlet) -> bool {
    scriptlet.requires_trust
        || std::iter::once(&scriptlet.name)
            .chain(&scriptlet.aliases)
            .any(|name| name.starts_with("trusted-"))
}

fn resource(scriptlet: Scriptlet) -> Resource {
    let mime = if scriptlet.name.ends_with(".fn") {
        MimeType::FnJavascript
    } else {
        MimeType::ApplicationJavascript
    };
    Resource {
        kind: ResourceType::Mime(mime),
        content: base64::engine::general_purpose::STANDARD.encode(&scriptlet.body),
        permission: if needs_trust(&scriptlet) {
            TRUSTED
        } else {
            PermissionMask::default()
        },
        name: scriptlet.name,
        aliases: scriptlet.aliases,
        dependencies: scriptlet.dependencies,
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) const LIBRARY: &str = r#"{
        "scriptlets": [
            { "name": "safe-self.fn", "aliases": [], "body": "function safeSelf(){return {};}",
              "dependencies": [], "executionWorld": "MAIN", "requiresTrust": false },
            { "name": "set-constant.js", "aliases": ["set.js"],
              "body": "function setConstant(a,b){safeSelf();}",
              "dependencies": ["safe-self.fn"], "executionWorld": "MAIN", "requiresTrust": false },
            { "name": "trusted-click-element.js", "aliases": [],
              "body": "function trustedClickElement(a){safeSelf();}",
              "dependencies": ["safe-self.fn"], "executionWorld": "MAIN", "requiresTrust": true },
            { "name": "trusted-prevent-xhr.js", "aliases": [],
              "body": "function trustedPreventXhr(a,b){safeSelf();}",
              "dependencies": ["safe-self.fn"], "executionWorld": "MAIN", "requiresTrust": false }
        ],
        "redirects": [
            { "name": "noop.js", "aliases": [], "body": "(function(){})();", "contentType": "application/javascript" }
        ]
    }"#;

    #[test]
    fn helpers_become_dependencies_and_trust_carries_over() {
        let resources = resources(LIBRARY).unwrap();
        assert_eq!(resources.len(), 4);

        let by_name = |name: &str| resources.iter().find(|r| r.name == name).unwrap();
        assert_eq!(
            by_name("safe-self.fn").kind,
            ResourceType::Mime(MimeType::FnJavascript)
        );
        let set_constant = by_name("set-constant.js");
        assert_eq!(
            set_constant.kind,
            ResourceType::Mime(MimeType::ApplicationJavascript)
        );
        assert_eq!(set_constant.aliases, vec!["set.js"]);
        assert_eq!(set_constant.dependencies, vec!["safe-self.fn"]);
        assert_eq!(set_constant.permission.to_bits(), 0);
        assert_eq!(
            by_name("trusted-click-element.js").permission.to_bits(),
            TRUSTED.to_bits()
        );
    }

    #[test]
    fn a_trusted_name_needs_trust_even_unmarked() {
        let resources = resources(LIBRARY).unwrap();
        let prevent_xhr = resources
            .iter()
            .find(|r| r.name == "trusted-prevent-xhr.js")
            .unwrap();
        assert_eq!(prevent_xhr.permission.to_bits(), TRUSTED.to_bits());
    }

    #[test]
    fn a_file_that_is_not_the_library_is_refused() {
        assert!(resources("[]").is_err());
        assert!(resources("<html>").is_err());
        assert!(resources(r#"{"scriptlets": []}"#).is_err());
    }

    #[test]
    fn a_library_that_would_load_only_in_part_is_refused() {
        let missing_helper = r#"{"scriptlets": [
            { "name": "set-constant.js", "body": "function setConstant(){}", "dependencies": ["safe-self.fn"] }
        ]}"#;
        assert!(resources(missing_helper).is_err());

        let taken_name = r#"{"scriptlets": [
            { "name": "set-constant.js", "body": "function setConstant(){}" },
            { "name": "set.js", "aliases": ["set-constant.js"], "body": "function set(){}" }
        ]}"#;
        assert!(resources(taken_name).is_err());
    }
}

//! uBO's `!#if` and `!#include` directives, which the engine reads as comments.
//! Conditions follow uBO's preparser for a desktop WebKit browser that is
//! neither uBO Lite nor able to filter HTML: of uBO's tokens only
//! `ext_ublock` holds.

/// uBO's known tokens; any other is unknown and keeps its branch, except
/// `cap_` ones, which do not hold.
const TOKENS: &[&str] = &[
    "ext_ublock",
    "ext_ubol",
    "ext_devbuild",
    "env_brave",
    "env_chromium",
    "env_edge",
    "env_firefox",
    "env_legacy",
    "env_mobile",
    "env_mv3",
    "env_safari",
    "cap_html_filtering",
    "cap_ipaddress",
    "false",
    "ext_abp",
    "adguard",
    "adguard_app_android",
    "adguard_app_cli",
    "adguard_app_ios",
    "adguard_app_mac",
    "adguard_app_windows",
    "adguard_ext_android_cb",
    "adguard_ext_chromium",
    "adguard_ext_chromium_mv3",
    "adguard_ext_edge",
    "adguard_ext_firefox",
    "adguard_ext_opera",
    "adguard_ext_safari",
];

pub struct Preprocessed {
    pub text: String,
    /// Paths as written, from kept branches only.
    pub includes: Vec<String>,
}

pub fn preprocess(list: &str) -> Preprocessed {
    let mut text = String::with_capacity(list.len());
    let mut includes = Vec::new();
    let mut branches: Vec<Option<bool>> = Vec::new();
    let mut dropping = 0usize;

    for line in list.lines() {
        if let Some(condition) = directive(line, "if") {
            let drops = evaluate(condition).map(|holds| !holds);
            dropping += usize::from(drops == Some(true));
            branches.push(drops);
            continue;
        }
        if directive(line, "else").is_some() {
            if let Some(Some(drops)) = branches.last_mut() {
                *drops = !*drops;
                if *drops {
                    dropping += 1;
                } else {
                    dropping -= 1;
                }
            }
            continue;
        }
        if directive(line, "endif").is_some() {
            dropping -= usize::from(branches.pop() == Some(Some(true)));
            continue;
        }
        if dropping > 0 {
            continue;
        }
        if let Some(path) = line.strip_prefix("!#include ") {
            if let Some(path) = path.split_whitespace().next() {
                includes.push(path.to_owned());
            }
            continue;
        }

        text.push_str(line);
        text.push('\n');
    }

    Preprocessed { text, includes }
}

/// Resolved against the parent's directory, as uBO does, and refused if it
/// would leave it, which uBO's `..` check alone does not ensure.
pub fn include_url(parent: &str, path: &str) -> Option<String> {
    let path = path.trim();
    if path.contains("..") || url::Url::parse(path).is_ok() {
        return None;
    }
    let directory = &parent[..=parent.rfind('/')?];
    let base = url::Url::parse(directory).ok()?;
    let resolved = url::Url::parse(&format!("{directory}{path}")).ok()?;
    (resolved.origin() == base.origin() && resolved.path().starts_with(base.path()))
        .then(|| resolved.into())
}

fn directive<'a>(line: &'a str, name: &str) -> Option<&'a str> {
    let rest = line.strip_prefix("!#")?.strip_prefix(name)?;
    match rest.chars().next() {
        None => Some(""),
        Some(c) if !c.is_alphanumeric() && c != '_' => Some(rest.trim()),
        Some(_) => None,
    }
}

/// Left to right, as uBO does, with `None` for anything it would not read.
/// Like uBO, an unknown first token reads as JavaScript's `undefined`.
fn evaluate(condition: &str) -> Option<bool> {
    let condition = condition
        .strip_prefix('(')
        .and_then(|inner| inner.strip_suffix(')'))
        .unwrap_or(condition);
    let mut words = condition.split(' ').filter(|word| !word.is_empty());
    let first = words.next()?;
    if first.starts_with(['|', '&']) {
        return None;
    }

    let mut holds = token_holds(first);
    while let Some(operator) = words.next() {
        let next = token_holds(words.next()?)?;
        holds = match operator {
            "&&" => holds.map(|holds| holds && next),
            "||" => Some(holds.unwrap_or(false) || next),
            _ => return None,
        };
    }

    holds
}

fn token_holds(token: &str) -> Option<bool> {
    let (negated, name) = match token.strip_prefix('!') {
        Some(name) => (true, name),
        None => (false, token),
    };
    let holds = if name == "ext_ublock" {
        true
    } else if TOKENS.contains(&name) || name.starts_with("cap_") {
        false
    } else {
        return None;
    };
    Some(holds != negated)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kept(list: &str) -> Vec<String> {
        preprocess(list).text.lines().map(str::to_owned).collect()
    }

    #[test]
    fn wis_keeps_the_branches_for_a_desktop_webkit_ublock() {
        let list = "always
!#if env_mobile
mobile
!#else
desktop
!#endif
!#if !ext_ubol
full
!#endif
!#if ext_ubol
lite
!#endif
!#if cap_html_filtering
html
!#endif
!#if cap_unheard_of
no_capability
!#endif
!#if unknown_token
unknown
!#endif
!#if env_firefox || !env_chromium
either
!#endif
!#if (env_safari && ext_ublock)
both
!#endif
!#if false
never
!#endif
!#if !false
not_never
!#endif
";
        assert_eq!(
            kept(list),
            [
                "always",
                "desktop",
                "full",
                "unknown",
                "either",
                "not_never"
            ]
        );
    }

    #[test]
    fn an_inner_branch_needs_its_outer_one() {
        let list = "!#if !env_mobile
!#if env_firefox
firefox
!#endif
desktop
!#endif
!#if env_mobile
!#if !env_firefox
mobile
!#endif
!#endif
";
        assert_eq!(kept(list), ["desktop"]);
    }

    #[test]
    fn a_malformed_condition_keeps_its_branch() {
        assert_eq!(
            kept("!#if env_mobile ||env_firefox\nkept\n!#endif\n"),
            ["kept"]
        );
        assert_eq!(kept("!#endif\n!#else\nstray\n"), ["stray"]);
    }

    /// uBO reads `undefined || x` as `x` and `undefined && x` as unreadable,
    /// and splits operators on spaces only.
    #[test]
    fn an_unknown_first_token_reads_as_ubo_reads_it() {
        let branches =
            |condition: &str| kept(&format!("!#if {condition}\nif\n!#else\nelse\n!#endif\n"));
        assert_eq!(branches("unknown || env_mobile"), ["else"]);
        assert_eq!(branches("unknown || ext_ublock"), ["if"]);
        assert_eq!(branches("unknown && ext_ublock"), ["if", "else"]);
        assert_eq!(branches("ext_ublock\t||\tenv_mobile"), ["if", "else"]);
        assert_eq!(branches("|| ext_ublock"), ["if", "else"]);
    }

    #[test]
    fn only_includes_in_kept_branches_are_followed() {
        let list = "rule
!#include filters-2024.txt
!#if env_mobile
!#include filters-mobile.txt
!#endif
";
        let processed = preprocess(list);
        assert_eq!(processed.includes, ["filters-2024.txt"]);
        assert_eq!(processed.text.lines().collect::<Vec<_>>(), ["rule"]);
    }

    #[test]
    fn an_include_stays_in_its_parent_s_directory() {
        let parent = "https://lists.example/filters/filters.txt";
        assert_eq!(
            include_url(parent, " filters-2024.txt ").as_deref(),
            Some("https://lists.example/filters/filters-2024.txt")
        );
        assert_eq!(
            include_url(parent, "sub/extra.txt").as_deref(),
            Some("https://lists.example/filters/sub/extra.txt")
        );
        assert_eq!(include_url(parent, "../secrets.txt"), None);
        assert_eq!(include_url(parent, "https://elsewhere.example/x.txt"), None);
        assert_eq!(include_url(parent, "data:text/plain,||x.test^"), None);
        assert_eq!(include_url(parent, "%2e%2e/%2e%2e/other/x.txt"), None);
        assert_eq!(include_url(parent, ".%2E/x.txt"), None);
        assert_eq!(
            include_url("https://lists.example", "evil.example/x.txt"),
            None
        );
        assert_eq!(
            include_url("https://lists.example?list=1", "evil.example/x.txt"),
            None
        );
        assert_eq!(
            include_url(parent, "./a.txt").as_deref(),
            Some("https://lists.example/filters/a.txt")
        );
    }
}

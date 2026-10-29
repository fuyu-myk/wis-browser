//! The language a page declares, from its untrusted `lang` attribute or
//! `Content-Language` meta.

/// Longest tag accepted.
const MAX_TAG_LEN: usize = 35;

/// Tags that name no translatable language.
const NO_LANGUAGE: [&str; 4] = ["und", "mul", "zxx", "mis"];

/// The page's declared language as a BCP 47 tag, or `None` when it declares
/// none or something that is not a language tag. A list keeps its first entry.
pub fn page_language(raw: &str) -> Option<String> {
    let tag = raw.split(',').next()?.trim().replace('_', "-");
    if tag.is_empty() || tag.len() > MAX_TAG_LEN {
        return None;
    }

    let mut subtags = tag.split('-');
    let primary = subtags.next()?;
    let primary_ok =
        (2..=3).contains(&primary.len()) && primary.bytes().all(|b| b.is_ascii_alphabetic());
    let rest_ok =
        subtags.all(|s| (1..=8).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_alphanumeric()));
    let named = !NO_LANGUAGE.contains(&primary.to_ascii_lowercase().as_str());

    (primary_ok && rest_ok && named).then_some(tag)
}

#[cfg(test)]
mod tests {
    use super::page_language;

    #[test]
    fn a_declared_language_comes_back_tidied() {
        assert_eq!(page_language("ja").as_deref(), Some("ja"));
        assert_eq!(page_language("  ja-JP ").as_deref(), Some("ja-JP"));
        assert_eq!(page_language("en_US").as_deref(), Some("en-US"));
        assert_eq!(page_language("zh-Hant-TW").as_deref(), Some("zh-Hant-TW"));
    }

    #[test]
    fn a_language_list_keeps_its_first_entry() {
        assert_eq!(page_language("de, en").as_deref(), Some("de"));
    }

    #[test]
    fn no_language_or_a_non_tag_is_none() {
        for raw in [
            "",
            "   ",
            "und",
            "ZXX",
            "x-klingon",
            "i-klingon-x",
            "<script>",
            "e",
            "english",
            "ja--JP",
        ] {
            assert_eq!(page_language(raw), None, "{raw:?}");
        }
        assert_eq!(page_language(&format!("ja-{}", "a".repeat(40))), None);
    }
}

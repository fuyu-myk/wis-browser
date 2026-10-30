//! The language a page declares, from its untrusted `lang` attribute or
//! `Content-Language` meta, and the language its text is in.

use serde::Deserialize;

/// Longest tag accepted.
const MAX_TAG_LEN: usize = 35;

/// Tags that name no translatable language.
const NO_LANGUAGE: [&str; 4] = ["und", "mul", "zxx", "mis"];

/// Letters a sample needs before the language detected in it counts.
const MIN_LETTERS: usize = 20;
/// Minimum confidence for the recogniser to count.
const MIN_CONFIDENCE: f64 = 0.5;
/// What it takes for the detected language to overrule a declared one.
const OVERRULE_LETTERS: usize = 100;
const OVERRULE_CONFIDENCE: f64 = 0.9;

/// A text sample's dominant language, empty when the recogniser can't tell,
/// and its probability.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Detected {
    pub language: String,
    pub confidence: f64,
}

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

fn primary(tag: &str) -> String {
    tag.split(['-', '_'])
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase()
}

/// The page's language: what it declares, unless a long sample is surely in
/// another language, or what its text is in when it declares nothing.
pub fn decide(
    declared: Option<String>,
    sample: &str,
    detected: Option<&Detected>,
) -> Option<String> {
    let letters = sample.chars().filter(|c| c.is_alphabetic()).count();
    let found = detected
        .filter(|found| {
            !found.language.is_empty() && !NO_LANGUAGE.contains(&found.language.as_str())
        })
        .filter(|_| letters >= MIN_LETTERS);
    match (declared, found) {
        (Some(declared), Some(found))
            if letters >= OVERRULE_LETTERS
                && found.confidence >= OVERRULE_CONFIDENCE
                && primary(&found.language) != primary(&declared) =>
        {
            Some(found.language.clone())
        }
        (Some(declared), _) => Some(declared),
        (None, Some(found)) if found.confidence >= MIN_CONFIDENCE => Some(found.language.clone()),
        (None, _) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{decide, page_language, Detected};

    fn detected(language: &str, confidence: f64) -> Detected {
        Detected {
            language: language.into(),
            confidence,
        }
    }

    #[test]
    fn a_page_declaring_nothing_takes_the_language_its_text_is_in() {
        let sample = "これは日本語で書かれた、ごく普通の文章です。";
        assert_eq!(
            decide(None, sample, Some(&detected("ja", 0.99))).as_deref(),
            Some("ja")
        );
        assert_eq!(decide(None, sample, Some(&detected("ja", 0.3))), None);
        assert_eq!(decide(None, "OK", Some(&detected("en", 0.99))), None);
        assert_eq!(decide(None, sample, Some(&detected("und", 1.0))), None);
        assert_eq!(decide(None, sample, None), None);
    }

    #[test]
    fn a_long_sample_surely_in_another_language_overrules_the_page() {
        let long = "日本語の文章。".repeat(30);
        assert_eq!(
            decide(Some("en".into()), &long, Some(&detected("ja", 0.98))).as_deref(),
            Some("ja")
        );
        assert_eq!(
            decide(Some("en".into()), &long, Some(&detected("ja", 0.6))).as_deref(),
            Some("en"),
            "not sure enough"
        );
        assert_eq!(
            decide(
                Some("en".into()),
                "日本語の文章。",
                Some(&detected("ja", 0.99))
            )
            .as_deref(),
            Some("en"),
            "too short"
        );
        assert_eq!(
            decide(
                Some("zh-TW".into()),
                &long,
                Some(&detected("zh-Hant", 0.99))
            )
            .as_deref(),
            Some("zh-TW"),
            "the same language"
        );
    }

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

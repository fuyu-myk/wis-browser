//! Repairs to a translation's inline elements: spaces moved off their edges,
//! and a lost element put back on its own words in the translation or,
//! failing that, the segment translated a piece at a time.

use std::future::Future;
use std::ops::Range;

use super::message::Run;

/// A segment with more pieces than this stays as it is.
pub const MAX_PIECES: usize = 12;

const STOP_WORDS: [&str; 12] = [
    "the", "and", "for", "with", "from", "that", "this", "are", "was", "you", "your", "our",
];
const MAX_NEEDLES: usize = 8;

/// Combining marks and joiners, which belong to the letter before them.
fn mark(c: char) -> bool {
    !c.is_alphanumeric()
        && matches!(c, '\u{0300}'..='\u{036F}' | '\u{0900}'..='\u{0963}' | '\u{0966}'..='\u{0DFF}'
            | '\u{0E00}'..='\u{0E4E}' | '\u{0E50}'..='\u{0EFF}' | '\u{200C}' | '\u{200D}')
}

fn word_char(c: char) -> bool {
    c.is_alphanumeric() || mark(c)
}

fn push_plain(out: &mut Vec<Run>, text: &str) {
    if text.is_empty() {
        return;
    }
    match out.last_mut() {
        Some(last) if last.tag.is_none() => last.text.push_str(text),
        _ => out.push(Run {
            text: text.to_string(),
            tag: None,
        }),
    }
}

/// Moves the spaces at a tagged run's edges out of it, so a link's
/// underline stops at its words.
pub fn tidy(runs: Vec<Run>) -> Vec<Run> {
    let mut out: Vec<Run> = Vec::with_capacity(runs.len() + 2);
    for run in runs {
        let core = run.text.trim();
        if run.tag.is_none() || core.is_empty() || core.len() == run.text.len() {
            match run.tag {
                None => push_plain(&mut out, &run.text),
                Some(_) => out.push(run),
            }
            continue;
        }

        let start = run.text.len() - run.text.trim_start().len();
        push_plain(&mut out, &run.text[..start]);
        out.push(Run {
            text: core.to_string(),
            tag: run.tag,
        });
        push_plain(&mut out, &run.text[start + core.len()..]);
    }

    out
}

/// The tags sent that did not come back, with the text each carried.
pub fn lost_tags<'a>(sent: &'a [Run], back: &[Run]) -> Vec<(u32, &'a str)> {
    sent.iter()
        .filter_map(|run| Some((run.tag?, run.text.as_str())))
        .filter(|(tag, _)| !back.iter().any(|back| back.tag == Some(*tag)))
        .collect()
}

/// What to look for: the whole piece, then its words, longest first.
fn needles(piece: &str) -> Vec<&str> {
    let common = |word: &str| STOP_WORDS.contains(&word.to_lowercase().as_str());
    let whole = piece.trim();
    let mut words: Vec<&str> = whole
        .split(|c: char| !word_char(c))
        .filter(|word| word.chars().count() >= 3 && !common(word))
        .collect();
    words.sort_by_key(|word| std::cmp::Reverse(word.chars().count()));

    let mut out = Vec::new();
    for needle in std::iter::once(whole)
        .filter(|whole| !common(whole))
        .chain(words)
    {
        if !needle.is_empty() && !out.contains(&needle) && out.len() < MAX_NEEDLES {
            out.push(needle);
        }
    }

    out
}

/// Where `needle` matches whole words of `hay`, ignoring case; one of five
/// letters or more may take up to three more as a stem. Without spaces
/// between words, anywhere it occurs.
fn find_word(hay: &str, needle: &str, spaced: bool) -> Option<Range<usize>> {
    let needle: Vec<char> = needle.chars().flat_map(char::to_lowercase).collect();
    let stem = needle.len() >= 5 && needle.last().is_some_and(|c| c.is_alphanumeric());
    if needle.is_empty() {
        return None;
    }

    let chars: Vec<(usize, char)> = hay.char_indices().collect();
    (0..chars.len()).find_map(|start| {
        if spaced && start > 0 && word_char(chars[start - 1].1) {
            return None;
        }

        let (mut at, mut rest) = (start, needle.as_slice());
        while !rest.is_empty() {
            let &(_, c) = chars.get(at)?;
            for lower in c.to_lowercase() {
                match rest.split_first() {
                    Some((&want, tail)) if want == lower => rest = tail,
                    _ => return None,
                }
            }
            at += 1;
        }

        while at < chars.len() && mark(chars[at].1) {
            at += 1;
        }

        let mut extra = 0;
        while spaced && at < chars.len() && word_char(chars[at].1) {
            if !stem || extra == 3 {
                return None;
            }
            at += 1;
            extra += 1;
        }

        Some(chars[start].0..chars.get(at).map_or(hay.len(), |c| c.0))
    })
}

/// Tags the words of the translation that each lost element, and translated
/// on its own as `found`. `None` when one cannot be placed.
pub fn anchor(mut runs: Vec<Run>, found: &[(u32, String)], spaced: bool) -> Option<Vec<Run>> {
    for (tag, piece) in found {
        let (at, range) = needles(piece).into_iter().find_map(|needle| {
            runs.iter()
                .enumerate()
                .filter(|(_, run)| run.tag.is_none())
                .find_map(|(i, run)| find_word(&run.text, needle, spaced).map(|range| (i, range)))
        })?;
        let text = std::mem::take(&mut runs[at].text);
        let split = [
            (&text[..range.start], None),
            (&text[range.clone()], Some(*tag)),
            (&text[range.end..], None),
        ]
        .into_iter()
        .filter(|(part, _)| !part.is_empty())
        .map(|(part, tag)| Run {
            text: part.to_string(),
            tag,
        });
        runs.splice(at..=at, split.collect::<Vec<_>>());
    }
    Some(runs)
}

pub fn pieces(runs: &[Run]) -> usize {
    runs.iter()
        .filter(|run| !run.text.trim().is_empty())
        .count()
}

/// Whether the language puts spaces between words.
pub fn spaced(lang: &str) -> bool {
    let primary = lang.split(['-', '_']).next().unwrap_or_default();
    !matches!(
        primary,
        "ja" | "zh" | "yue" | "th" | "lo" | "km" | "my" | "bo"
    )
}

fn gap(before: &str, after: &str) -> bool {
    let (Some(last), Some(first)) = (before.chars().last(), after.chars().next()) else {
        return false;
    };
    !last.is_whitespace()
        && !first.is_whitespace()
        && !"([{“‘「『（¿¡$".contains(last)
        && !".,;:!?)]}%”’…。、」』）".contains(first)
}

/// Translates each run on its own, keeping its tag, with a space between
/// pieces whose words would otherwise touch. `None` when a piece fails or
/// comes back blank.
pub async fn piecewise<F, Fut>(runs: &[Run], spaced: bool, mut translate: F) -> Option<Vec<Run>>
where
    F: FnMut(String) -> Fut,
    Fut: Future<Output = Option<String>>,
{
    let mut out: Vec<Run> = Vec::with_capacity(runs.len());
    for run in runs {
        let mut text = if run.text.trim().is_empty() {
            run.text.clone()
        } else {
            translate(run.text.clone())
                .await
                .filter(|text| !text.trim().is_empty())?
        };
        if let Some(prev) = out
            .last_mut()
            .filter(|prev| spaced && gap(&prev.text, &text))
        {
            if prev.tag.is_none() {
                prev.text.push(' ');
            } else {
                text.insert(0, ' ');
            }
        }
        out.push(Run { text, tag: run.tag });
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(text: &str, tag: Option<u32>) -> Run {
        Run {
            text: text.into(),
            tag,
        }
    }

    #[test]
    fn spaces_at_a_tagged_run_s_edges_move_out_of_it() {
        assert_eq!(
            tidy(vec![
                run("by the ", None),
                run("Digital Agency ", Some(0)),
                run("and free", None),
            ]),
            vec![
                run("by the ", None),
                run("Digital Agency", Some(0)),
                run(" and free", None),
            ]
        );
        assert_eq!(
            tidy(vec![run(" here ", Some(0)), run(" ", Some(1))]),
            vec![
                run(" ", None),
                run("here", Some(0)),
                run(" ", None),
                run(" ", Some(1)),
            ]
        );
    }

    #[test]
    fn a_translation_that_dropped_an_inline_element_is_caught() {
        let sent = [
            run("詳しくは", None),
            run("こちら", Some(0)),
            run("を参照", None),
        ];
        assert!(lost_tags(&sent, &[run("See ", None), run("here", Some(0))]).is_empty());
        assert_eq!(
            lost_tags(&sent, &[run("See this article", None)]),
            vec![(0, "こちら")]
        );
    }

    #[test]
    fn a_lost_element_goes_back_on_its_own_words() {
        let runs = vec![run("For details, please click here.", None)];
        assert_eq!(
            anchor(runs, &[(0, "here".into())], true),
            Some(vec![
                run("For details, please click ", None),
                run("here", Some(0)),
                run(".", None),
            ])
        );
    }

    #[test]
    fn a_lost_element_finds_its_longest_word_or_its_stem() {
        let runs = vec![
            run("Operated by the ", None),
            run("Digital Agency", Some(0)),
            run(" and free; past notices are archived.", None),
        ];
        assert_eq!(
            anchor(
                runs,
                &[(1, "Free of charge".into()), (2, "an archive".into())],
                true
            ),
            Some(vec![
                run("Operated by the ", None),
                run("Digital Agency", Some(0)),
                run(" and ", None),
                run("free", Some(1)),
                run("; past notices are ", None),
                run("archived", Some(2)),
                run(".", None),
            ])
        );
    }

    #[test]
    fn a_lost_element_is_not_put_inside_another_word_or_element() {
        let inside_a_word = vec![run("There is more.", None)];
        assert_eq!(anchor(inside_a_word, &[(0, "here".into())], true), None);
        let taken = vec![run("here", Some(1)), run(" and here", None)];
        assert_eq!(
            anchor(taken, &[(0, "here".into())], true),
            Some(vec![
                run("here", Some(1)),
                run(" and ", None),
                run("here", Some(0)),
            ])
        );
    }

    #[test]
    fn a_short_or_common_piece_only_matches_a_whole_word() {
        for (hay, piece) in [
            ("see article 1990.", "1"),
            ("the agency and free", "A"),
            ("the hereby clause", "here"),
            ("the terms", "The"),
        ] {
            assert_eq!(
                anchor(vec![run(hay, None)], &[(0, piece.into())], true),
                None,
                "{piece} in {hay}"
            );
        }
        assert_eq!(
            anchor(vec![run("see article 1.", None)], &[(0, "1".into())], true),
            Some(vec![
                run("see article ", None),
                run("1", Some(0)),
                run(".", None),
            ])
        );
    }

    #[test]
    fn a_match_takes_the_marks_that_belong_to_its_last_letter() {
        let thai = vec![run("ดูที่นี่ครับ", None)];
        assert_eq!(
            anchor(thai, &[(0, "นี".into())], false),
            Some(vec![run("ดูที่", None), run("นี่", Some(0)), run("ครับ", None),])
        );
    }

    #[test]
    fn without_spaces_a_lost_element_matches_anywhere() {
        let runs = vec![run("詳細はここをクリック", None)];
        assert_eq!(
            anchor(runs, &[(0, "ここ".into())], false),
            Some(vec![
                run("詳細は", None),
                run("ここ", Some(0)),
                run("をクリック", None),
            ])
        );
    }

    #[test]
    fn pieces_get_a_space_only_where_words_would_touch() {
        assert!(gap("For details", "here"));
        assert!(gap("For details,", "here"));
        assert!(!gap("For details ", "here"));
        assert!(!gap("here", ", please"));
        assert!(!gap("(", "here"));
        assert!(!gap("¿", "Dónde"));
        assert!(!gap("", "here"));
    }

    #[test]
    fn languages_written_without_spaces_get_none() {
        assert!(spaced("en"));
        assert!(spaced("fr-CA"));
        assert!(!spaced("ja"));
        assert!(!spaced("zh-Hant"));
        assert!(!spaced("th"));
    }

    #[tokio::test]
    async fn a_segment_goes_piece_by_piece_keeping_its_tags() {
        let sent = [
            run("詳しくは", None),
            run("こちら", Some(0)),
            run("の記事を参照", None),
            run(" ", None),
        ];
        let fake = |text: String| async move {
            match text.as_str() {
                "詳しくは" => Some("For details".to_string()),
                "こちら" => Some("here".to_string()),
                "の記事を参照" => Some("see the article".to_string()),
                _ => None,
            }
        };
        assert_eq!(
            piecewise(&sent, true, fake).await,
            Some(vec![
                run("For details ", None),
                run("here", Some(0)),
                run(" see the article", None),
                run(" ", None),
            ])
        );
        assert_eq!(
            piecewise(&sent[..3], false, fake).await,
            Some(vec![
                run("For details", None),
                run("here", Some(0)),
                run("see the article", None),
            ])
        );
    }

    #[tokio::test]
    async fn a_failed_or_blank_piece_leaves_the_segment_alone() {
        let sent = [run("詳しくは", None), run("こちら", Some(0))];
        let failing =
            |text: String| async move { (text == "詳しくは").then(|| "For details".to_string()) };
        assert_eq!(piecewise(&sent, true, failing).await, None);
        let blank = |text: String| async move {
            Some(if text == "こちら" {
                String::new()
            } else {
                text
            })
        };
        assert_eq!(piecewise(&sent, true, blank).await, None);
    }
}

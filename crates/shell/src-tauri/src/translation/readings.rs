//! A Japanese page gives a term's reading in parentheses after it, as in
//! "<b>二次方程式</b>（にじほうていしき、英: quadratic equation）". The reading
//! is for Japanese readers, and the translator gives it back in Japanese
//! ("Japanese:二次方程式"), so it is left out of what is sent.

use super::message::Run;

/// Hiragana words a parenthesis may open with.
const WORDS: [&str; 12] = [
    "または",
    "あるいは",
    "もしくは",
    "ただし",
    "なお",
    "つまり",
    "すなわち",
    "および",
    "ないし",
    "いわゆる",
    "はい",
    "いいえ",
];

fn han(c: char) -> bool {
    matches!(c, '\u{4E00}'..='\u{9FFF}' | '\u{3400}'..='\u{4DBF}' | '\u{F900}'..='\u{FAFF}' | '々')
}

fn hiragana(c: char) -> bool {
    matches!(c, '\u{3041}'..='\u{3096}' | '\u{309D}'..='\u{309F}')
}

/// Whether `item` reads a term of `kanji` kanji: hiragana only, at least a
/// kana a kanji, and not a word of its own.
fn reading(item: &str, kanji: usize) -> bool {
    let item = item.trim_matches([' ', '\u{3000}']);
    item.chars().filter(|&c| hiragana(c)).count() >= kanji
        && item
            .chars()
            .all(|c| hiragana(c) || matches!(c, 'ー' | '・' | ' ' | '\u{3000}' | '-' | '‐' | '－'))
        && !WORDS.contains(&item)
}

/// The text after a "（" without the readings of a term of `kanji` kanji it
/// opens with, and without the parenthesis when it held nothing else; `None`
/// when it opens with none, or lists words in kana ("ひらがな、カタカナ").
fn without_reading(rest: &str, kanji: usize) -> Option<String> {
    let close = rest.find('）');
    let inner = &rest[..close.unwrap_or(rest.len())];
    let mut cut = 0;
    for item in inner.split_inclusive(['、', '，']) {
        if !reading(item.trim_end_matches(['、', '，']), kanji) {
            break;
        }
        cut += item.len();
    }

    let next = inner[cut..]
        .split(['、', '，'])
        .next()
        .unwrap_or_default()
        .trim();
    let kana = |c: char| hiragana(c) || matches!(c, '\u{30A1}'..='\u{30FA}' | 'ー' | '・');
    let listed = !next.is_empty() && next.chars().all(kana);
    match close {
        _ if cut == 0 || listed => None,
        Some(close) if cut == close => Some(rest[close + '）'.len_utf8()..].to_string()),
        _ => Some(format!("（{}", &rest[cut..])),
    }
}

/// The runs to send with the readings after styled terms left out, or `None`
/// when there are none or the page is not Japanese. Links are not terms, so
/// a list after one ("文字（ひらがな、カタカナ）") stays.
pub fn unread(runs: &[Run], needed: &[u32], source: &str) -> Option<Vec<Run>> {
    if source.split(['-', '_']).next() != Some("ja") {
        return None;
    }

    let styled = |run: &Run| run.tag.is_some_and(|tag| !needed.contains(&tag));
    let mut out: Vec<Run> = Vec::with_capacity(runs.len());
    let mut changed = false;
    let mut at = 0;
    while at < runs.len() {
        let end = at
            + runs[at..]
                .iter()
                .take_while(|run| run.tag.is_none())
                .count();
        let kanji = out
            .last()
            .filter(|&run| styled(run))
            .map_or(0, |run| run.text.chars().filter(|&c| han(c)).count());
        if end > at && kanji > 0 {
            let text: String = runs[at..end].iter().map(|run| run.text.as_str()).collect();
            let rest = text.strip_prefix('（');
            if let Some(rest) = rest.and_then(|rest| without_reading(rest, kanji)) {
                changed = true;
                if !rest.is_empty() {
                    out.push(Run {
                        text: rest,
                        tag: None,
                    });
                }

                at = end;
                continue;
            }
        }

        out.push(runs[at].clone());
        at += 1;
    }

    changed.then_some(out)
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

    fn text(runs: &[Run]) -> String {
        runs.iter().map(|run| run.text.as_str()).collect()
    }

    #[test]
    fn a_reading_after_a_styled_term_is_left_out() {
        let sent = [
            run("二次方程式", Some(0)),
            run("（にじほうていしき、", None),
            run("英", Some(1)),
            run(": quadratic equation）とは、", None),
        ];
        let out = unread(&sent, &[1], "ja").unwrap();
        assert_eq!(text(&out), "二次方程式（英: quadratic equation）とは、");
        assert_eq!(out[2], run("英", Some(1)));
    }

    #[test]
    fn a_parenthesis_of_readings_alone_goes_whole() {
        let sent = [
            run("日本国", Some(0)),
            run("（にほんこく、にっぽん-）、または", None),
            run("日本", Some(1)),
            run("（にほん、にっぽん", None),
            run("）は、国家。", None),
        ];
        let out = unread(&sent, &[], "ja-JP").unwrap();
        assert_eq!(text(&out), "日本国、または日本は、国家。");
        let name = [
            run("夏目 漱石", Some(0)),
            run("（なつめ そうせき、1867年 - 1916年）は", None),
        ];
        assert_eq!(
            text(&unread(&name, &[], "ja").unwrap()),
            "夏目 漱石（1867年 - 1916年）は"
        );
    }

    #[test]
    fn other_parentheses_and_other_languages_are_left_alone() {
        let linked = [run("文字", Some(0)), run("（ひらがな、カタカナ）", None)];
        assert_eq!(unread(&linked, &[0], "ja"), None, "after a link");
        let loose = [run("文字（ひらがな、カタカナ）", None)];
        assert_eq!(unread(&loose, &[], "ja"), None, "after no styled term");
        let meaning = [run("用語", Some(0)), run("（漢字で書く）", None)];
        assert_eq!(unread(&meaning, &[], "ja"), None, "not a reading");
        let katakana = [run("計算機", Some(0)), run("（コンピュータ）", None)];
        assert_eq!(unread(&katakana, &[], "ja"), None, "a gloss");
        for (term, after) in [
            ("回答", "（はい、いいえ）を選ぶ"),
            ("文字", "（ひらがな、カタカナ）"),
            ("注意", "（ただし、例外あり）"),
            ("東京", "（または、江戸）"),
            ("ねこ", "（いぬ、とり）"),
            ("日本国", "（に、国）"),
        ] {
            let styled = [run(term, Some(0)), run(after, None)];
            assert_eq!(unread(&styled, &[], "ja"), None, "{term}{after}");
        }
        let chinese = [run("方程", Some(0)), run("（にじ）", None)];
        assert_eq!(unread(&chinese, &[], "zh"), None, "not Japanese");
    }
}

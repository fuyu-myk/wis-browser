//! Whether a pair's high-fidelity model keeps inline elements, learned from
//! its answers. It is translated in low latency throughout.

use std::collections::{BTreeSet, HashMap};
use std::sync::{Mutex, MutexGuard, PoisonError};

use super::message::{Block, Run};

const ANSWERS_TO_DECIDE: usize = 3;
const TAGS_TO_DECIDE: usize = 8;

/// What each pair's model was seen to do, until Wis quits.
static PAIRS: Mutex<Option<HashMap<String, Keeps>>> = Mutex::new(None);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Keeps {
    Unknown { answers: usize, lost: usize },
    Some,
    None,
}

impl Keeps {
    fn after(self, sent: usize, kept: usize) -> Keeps {
        match self {
            Keeps::Unknown { .. } if kept > 0 => Keeps::Some,
            Keeps::Unknown { answers, lost } => {
                let (answers, lost) = (answers + 1, lost + sent);
                if answers >= ANSWERS_TO_DECIDE && lost >= TAGS_TO_DECIDE {
                    Keeps::None
                } else {
                    Keeps::Unknown { answers, lost }
                }
            }
            settled => settled,
        }
    }
}

fn pairs() -> MutexGuard<'static, Option<HashMap<String, Keeps>>> {
    PAIRS.lock().unwrap_or_else(PoisonError::into_inner)
}

fn pair(source: &str, target: &str) -> String {
    format!("{source}>{target}").to_ascii_lowercase()
}

/// The needed tags `block` sent, objects aside, and how many of them came
/// back in `back`, or `None` when it sent none or has no words outside them.
fn tags_kept(block: &Block, back: &[Run]) -> Option<(usize, usize)> {
    let worded = block
        .runs
        .iter()
        .any(|run| run.tag.is_none() && run.text.chars().any(char::is_alphanumeric));
    let tags = |runs: &[Run]| -> BTreeSet<u32> {
        runs.iter()
            .filter_map(|run| run.tag)
            .filter(|tag| block.needed.contains(tag) && !block.objects.contains(tag))
            .collect()
    };
    let sent = tags(&block.runs);
    let kept = sent.intersection(&tags(back)).count();
    (worded && !sent.is_empty()).then_some((sent.len(), kept))
}

/// Notes what a high-fidelity answer `back` to `block` kept.
pub fn record(source: &str, target: &str, block: &Block, back: &[Run]) {
    let Some((sent, kept)) = tags_kept(block, back) else {
        return;
    };
    let mut pairs = pairs();
    let keeps = pairs
        .get_or_insert_with(HashMap::new)
        .entry(pair(source, target))
        .or_insert(Keeps::Unknown {
            answers: 0,
            lost: 0,
        });
    let next = keeps.after(sent, kept);
    if next == Keeps::None && *keeps != Keeps::None {
        log::debug!("{source} into {target} keeps no inline elements, using low latency");
    }
    *keeps = next;
}

/// Whether the pair's high-fidelity model was seen to keep no inline elements.
pub fn keeps_none(source: &str, target: &str) -> bool {
    pairs()
        .as_ref()
        .and_then(|pairs| pairs.get(&pair(source, target)))
        == Some(&Keeps::None)
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
    fn a_pair_keeps_none_once_enough_answers_and_tags_are_lost_and_none_kept() {
        let start = Keeps::Unknown {
            answers: 0,
            lost: 0,
        };
        assert_eq!(
            start.after(23, 0),
            Keeps::Unknown {
                answers: 1,
                lost: 23
            },
            "one segment is not enough"
        );
        let mut keeps = start;
        for _ in 0..7 {
            keeps = keeps.after(1, 0);
        }
        assert_eq!(
            keeps,
            Keeps::Unknown {
                answers: 7,
                lost: 7
            },
            "nor are lone links"
        );
        assert_eq!(keeps.after(1, 0), Keeps::None);
        assert_eq!(start.after(1, 0).after(1, 0).after(23, 0), Keeps::None);
    }

    #[test]
    fn one_kept_tag_settles_that_the_pair_keeps_them() {
        let unknown = Keeps::Unknown {
            answers: 5,
            lost: 7,
        };
        assert_eq!(unknown.after(3, 1), Keeps::Some);
        assert_eq!(Keeps::Some.after(20, 0), Keeps::Some);
        assert_eq!(Keeps::None.after(2, 2), Keeps::None);
    }

    #[test]
    fn the_count_takes_each_needed_tag_once_but_objects() {
        let block = Block {
            id: 1,
            runs: vec![
                run("Окно ", None),
                run("Firefox", Some(0)),
                run(" и ", None),
                run("\u{FFFC}", Some(1)),
                run(" и ", None),
                run("Firefox", Some(0)),
                run(" версии ", None),
                run("57", Some(2)),
            ],
            main: true,
            needed: vec![0, 1],
            objects: vec![1],
            long: false,
        };
        let back = [
            run("Firefox", Some(0)),
            run(" window, version ", None),
            run("\u{FFFC}", Some(1)),
        ];
        assert_eq!(tags_kept(&block, &back), Some((1, 1)));
        let plain = [run("Firefox window", None)];
        assert_eq!(tags_kept(&block, &plain), Some((1, 0)));
    }

    #[test]
    fn a_segment_with_no_needed_tags_or_no_words_of_its_own_tells_nothing() {
        let styled = Block {
            id: 1,
            runs: vec![run("Очень ", None), run("важно", Some(0))],
            main: true,
            needed: vec![],
            objects: vec![],
            long: false,
        };
        assert_eq!(tags_kept(&styled, &[run("Very important", None)]), None);
        let links = Block {
            runs: vec![
                run("Главная", Some(0)),
                run(" · ", None),
                run("Новости", Some(1)),
            ],
            needed: vec![0, 1],
            ..styled
        };
        assert_eq!(tags_kept(&links, &[run("Home · News", None)]), None);
    }
}

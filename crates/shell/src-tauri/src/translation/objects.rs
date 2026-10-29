//! Images, icons and formulas inside a segment travel as placeholders. The
//! low-latency model keeps U+FFFC where it stood; high fidelity drops it, so
//! there an object goes as a bullet marked not to translate.

use super::message::Run;
use super::pieces::{push_plain, tag_within};

pub const OBJECT: &str = "\u{FFFC}";
pub const HELD: &str = "•";
const SPACED: &str = " \u{FFFC} ";

/// The runs to send to high fidelity, each object as the bullet.
pub fn held(runs: &[Run], objects: &[u32]) -> Vec<Run> {
    runs.iter()
        .map(|run| match run.tag {
            Some(tag) if objects.contains(&tag) => Run {
                text: HELD.into(),
                tag: run.tag,
            },
            _ => run.clone(),
        })
        .collect()
}

/// Puts the objects back on what stands for them in a reply to `sent`. An
/// object's tag on other words gives them back. Stray placeholders are removed.
pub fn place(runs: Vec<Run>, sent: &[Run], objects: &[u32], marker: &str) -> Vec<Run> {
    if objects.is_empty() {
        return runs;
    }

    let object = |run: &Run| run.tag.is_some_and(|tag| objects.contains(&tag));
    let mut out: Vec<Run> = Vec::with_capacity(runs.len() + 2);
    for run in runs {
        if run.tag.is_none() || (object(&run) && run.text.trim() != marker) {
            push_plain(&mut out, &run.text);
        } else {
            out.push(run);
        }
    }

    let missing: Vec<u32> = objects
        .iter()
        .copied()
        .filter(|&tag| !out.iter().any(|run| run.tag == Some(tag)))
        .collect();
    let theirs = sent
        .iter()
        .any(|run| !object(run) && run.text.contains(marker));
    let mut found = out
        .iter()
        .enumerate()
        .filter(|(_, run)| run.tag.is_none())
        .flat_map(|(at, run)| {
            run.text
                .match_indices(marker)
                .map(move |(start, _)| (at, start))
        });
    if let ([tag], false, Some((at, start)), None) =
        (missing.as_slice(), theirs, found.next(), found.next())
    {
        tag_within(&mut out, at, start..start + marker.len(), *tag);
    }

    for run in out.iter_mut().filter(|run| run.tag.is_none()) {
        if run.text.contains(OBJECT) {
            run.text = run.text.replace(SPACED, " ").replace(OBJECT, "");
        }
    }

    out
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

    fn sent_one() -> Vec<Run> {
        vec![
            run("右上の", None),
            run(OBJECT, Some(0)),
            run("ボタン", None),
        ]
    }

    #[test]
    fn high_fidelity_gets_each_object_as_a_bullet() {
        let sent = [
            run("右上の", None),
            run(OBJECT, Some(0)),
            run("ボタン", Some(1)),
        ];
        assert_eq!(
            held(&sent, &[0]),
            vec![
                run("右上の", None),
                run("•", Some(0)),
                run("ボタン", Some(1))
            ]
        );
    }

    #[test]
    fn an_object_back_untagged_goes_on_its_marker() {
        let back = vec![run("Press the \u{FFFC} button.", None)];
        assert_eq!(
            place(back, &sent_one(), &[0], OBJECT),
            vec![
                run("Press the ", None),
                run(OBJECT, Some(0)),
                run(" button.", None)
            ]
        );
        let back = vec![run("Items with •", None), run(" are required.", Some(1))];
        assert_eq!(
            place(back, &sent_one(), &[0], HELD),
            vec![
                run("Items with ", None),
                run("•", Some(0)),
                run(" are required.", Some(1))
            ]
        );
    }

    #[test]
    fn only_the_marker_of_the_model_that_answered_counts() {
        let sent = [
            run("2026年10月 • ", None),
            run(OBJECT, Some(0)),
            run(" 5分", None),
        ];
        let back = vec![run("October 2026 • \u{FFFC} 5 min", None)];
        assert_eq!(
            place(back, &sent, &[0], OBJECT),
            vec![
                run("October 2026 • ", None),
                run(OBJECT, Some(0)),
                run(" 5 min", None)
            ]
        );
        let back = vec![run("October 2026 • 5 min", None)];
        assert_eq!(place(back.clone(), &sent, &[0], HELD), back);
    }

    #[test]
    fn an_object_tag_on_other_words_gives_them_back() {
        let back = vec![run("Press the ", None), run("button •", Some(0))];
        assert_eq!(
            place(back, &sent_one(), &[0], HELD),
            vec![run("Press the button ", None), run("•", Some(0))]
        );
    }

    #[test]
    fn objects_that_cannot_be_told_apart_stay_lost() {
        let sent = [
            run(OBJECT, Some(2)),
            run("の中の", None),
            run(OBJECT, Some(5)),
        ];
        let back = vec![run("the \u{FFFC} inside \u{FFFC}", None)];
        assert_eq!(
            place(back, &sent, &[2, 5], OBJECT),
            vec![run("the inside ", None)]
        );
    }

    #[test]
    fn an_object_back_on_its_tag_or_lost_is_left_as_it_is() {
        let tagged = vec![
            run("Press the ", None),
            run(" • ", Some(0)),
            run(" button", None),
        ];
        assert_eq!(place(tagged.clone(), &sent_one(), &[0], HELD), tagged);
        let lost = vec![run("Press the button.", None)];
        assert_eq!(place(lost.clone(), &sent_one(), &[0], HELD), lost);
        let inside = vec![run("a • b", Some(1))];
        assert_eq!(place(inside.clone(), &sent_one(), &[0], HELD), inside);
    }

    #[test]
    fn a_segment_without_objects_is_left_as_it_came() {
        let back = vec![run("A \u{FFFC} ", None), run("b", None)];
        assert_eq!(place(back.clone(), &[], &[], OBJECT), back);
    }

    #[test]
    fn a_stray_placeholder_is_removed() {
        let back = vec![
            run("Press ", None),
            run(OBJECT, Some(0)),
            run(" the \u{FFFC} button\u{FFFC}", None),
        ];
        assert_eq!(
            place(back, &sent_one(), &[0], OBJECT),
            vec![
                run("Press ", None),
                run(OBJECT, Some(0)),
                run(" the button", None)
            ]
        );
    }
}

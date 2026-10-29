//! What `page.js` posts to `wisTranslate`, and the order its segments are
//! translated in. The page's text is untrusted, so every message is capped.

use std::collections::{HashSet, VecDeque};

use serde::{Deserialize, Serialize};

/// `page.js` sends at most 24 segments of 4000 characters at a time, and
/// numbers at most 2000 segments a page; these caps leave room above that.
pub const MAX_MESSAGE_BYTES: usize = 512 * 1024;
pub const MAX_BLOCKS: usize = 32;
pub const MAX_RUNS: usize = 128;
pub const MAX_BLOCK_BYTES: usize = 16 * 1024;
const MAX_BLOCK_ID: u32 = 4096;
const MAX_LANG_LEN: usize = 64;

/// A piece of a segment's text, tagged with the inline element it sits in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Run {
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<u32>,
}

/// One segment of the page, numbered in document order.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Block {
    pub id: u32,
    pub runs: Vec<Run>,
    /// Whether it is in the page's main content.
    #[serde(default)]
    pub main: bool,
}

impl Block {
    /// Whether the segment is plain text, with no inline elements.
    pub fn plain(&self) -> bool {
        self.runs.iter().all(|run| run.tag.is_none())
    }

    fn within_caps(&self) -> bool {
        let bytes: usize = self.runs.iter().map(|run| run.text.len()).sum();
        self.id < MAX_BLOCK_ID && self.runs.len() <= MAX_RUNS && bytes <= MAX_BLOCK_BYTES
    }
}

/// A message from the page, for the translation generation `gen` it belongs to.
#[derive(Debug, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PageMessage {
    /// The page's declared language, raw.
    Start { gen: u64, lang: String },
    /// Segments that came into view. Those that came into view together
    /// share a wave, numbered upwards, and may span several messages.
    Blocks {
        gen: u64,
        wave: u64,
        blocks: Vec<Block>,
    },
}

/// The message in `body`, or `None` when it is malformed or over a cap. A
/// segment over a cap is dropped alone.
pub fn parse_message(body: &str) -> Option<PageMessage> {
    if body.len() > MAX_MESSAGE_BYTES {
        return None;
    }
    match serde_json::from_str(body).ok()? {
        PageMessage::Start { gen, lang } => {
            (lang.len() <= MAX_LANG_LEN).then_some(PageMessage::Start { gen, lang })
        }
        PageMessage::Blocks {
            gen,
            wave,
            mut blocks,
        } => {
            if blocks.len() > MAX_BLOCKS {
                return None;
            }
            blocks.retain(Block::within_caps);
            Some(PageMessage::Blocks { gen, wave, blocks })
        }
    }
}

/// Segments waiting for translation. The newest wave is what just came into
/// view; a segment is queued once per translation.
#[derive(Debug, Default)]
pub struct Queue {
    waves: VecDeque<(u64, VecDeque<Block>)>,
    queued: HashSet<u32>,
}

impl Queue {
    pub fn push_batch(&mut self, wave: u64, blocks: Vec<Block>) {
        let fresh: VecDeque<Block> = blocks
            .into_iter()
            .filter(|block| self.queued.insert(block.id))
            .collect();
        if fresh.is_empty() {
            return;
        }
        match self.waves.front_mut() {
            Some((front, batch)) if *front == wave => batch.extend(fresh),
            _ => self.waves.push_front((wave, fresh)),
        }
    }

    /// The next segment of the newest wave: the first that `prefer` picks
    /// from the same part of the page as the first in line, or else the
    /// first in line.
    pub fn next(&mut self, prefer: impl Fn(&Block) -> bool) -> Option<Block> {
        loop {
            let (_, batch) = self.waves.front_mut()?;
            let Some(main) = batch.front().map(|head| head.main) else {
                self.waves.pop_front();
                continue;
            };
            let at = batch
                .iter()
                .take_while(|block| block.main == main)
                .position(&prefer)
                .unwrap_or(0);
            return batch.remove(at);
        }
    }
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

    fn block(id: u32) -> Block {
        Block {
            id,
            runs: vec![run(&format!("段落{id}"), None)],
            main: false,
        }
    }

    fn in_main(block: Block) -> Block {
        Block {
            main: true,
            ..block
        }
    }

    #[test]
    fn a_start_message_carries_its_generation_and_language() {
        assert_eq!(
            parse_message(r#"{"kind":"start","gen":3,"lang":"ja"}"#),
            Some(PageMessage::Start {
                gen: 3,
                lang: "ja".into()
            })
        );
    }

    #[test]
    fn a_blocks_message_carries_tagged_runs() {
        assert_eq!(
            parse_message(
                r#"{"kind":"blocks","gen":3,"wave":2,"blocks":[{"id":0,"runs":[{"text":"詳しくは"},{"text":"こちら","tag":0}],"main":true}]}"#
            ),
            Some(PageMessage::Blocks {
                gen: 3,
                wave: 2,
                blocks: vec![Block {
                    id: 0,
                    runs: vec![run("詳しくは", None), run("こちら", Some(0))],
                    main: true,
                }]
            })
        );
    }

    #[test]
    fn malformed_or_oversized_messages_are_dropped() {
        let long_lang = format!(r#"{{"kind":"start","gen":1,"lang":"{}"}}"#, "a".repeat(100));
        let many_blocks = format!(
            r#"{{"kind":"blocks","gen":1,"wave":1,"blocks":[{}]}}"#,
            vec![r#"{"id":0,"runs":[{"text":"x"}]}"#; MAX_BLOCKS + 1].join(",")
        );
        for body in [
            "not json",
            r#"{"kind":"other","gen":1}"#,
            r#"{"kind":"start","lang":"ja"}"#,
            long_lang.as_str(),
            many_blocks.as_str(),
        ] {
            assert_eq!(parse_message(body), None, "{}", &body[..body.len().min(60)]);
        }
        assert_eq!(parse_message(&"x".repeat(MAX_MESSAGE_BYTES + 1)), None);
    }

    #[test]
    fn a_segment_over_a_cap_is_dropped_and_the_rest_of_its_batch_kept() {
        let many_runs = vec![r#"{"text":"x"}"#; MAX_RUNS + 1].join(",");
        let long_text = "あ".repeat(MAX_BLOCK_BYTES / 3 + 1);
        let body = format!(
            r#"{{"kind":"blocks","gen":1,"wave":1,"blocks":[{{"id":0,"runs":[{many_runs}]}},{{"id":1,"runs":[{{"text":"{long_text}"}}]}},{{"id":99999,"runs":[{{"text":"x"}}]}},{{"id":2,"runs":[{{"text":"段落2"}}]}}]}}"#
        );
        assert_eq!(
            parse_message(&body),
            Some(PageMessage::Blocks {
                gen: 1,
                wave: 1,
                blocks: vec![block(2)]
            })
        );
    }

    fn tagged(id: u32) -> Block {
        Block {
            id,
            runs: vec![run("詳しくは", None), run("こちら", Some(0))],
            main: false,
        }
    }

    fn order(queue: &mut Queue) -> Vec<u32> {
        std::iter::from_fn(|| queue.next(|_| false).map(|b| b.id)).collect()
    }

    #[test]
    fn the_newest_wave_goes_first_in_its_own_order() {
        let mut queue = Queue::default();
        queue.push_batch(1, vec![block(0), block(1)]);
        queue.push_batch(2, vec![block(5), block(6)]);
        assert_eq!(order(&mut queue), vec![5, 6, 0, 1]);
    }

    #[test]
    fn a_wave_sent_in_several_messages_keeps_its_order() {
        let mut queue = Queue::default();
        queue.push_batch(1, vec![block(0), block(1)]);
        queue.push_batch(1, vec![block(2), block(3)]);
        assert_eq!(queue.next(|_| false).map(|b| b.id), Some(0));
        queue.push_batch(2, vec![block(9)]);
        queue.push_batch(2, vec![block(10)]);
        assert_eq!(order(&mut queue), vec![9, 10, 1, 2, 3]);
    }

    #[test]
    fn preferred_segments_go_first_within_their_wave_only() {
        let mut queue = Queue::default();
        queue.push_batch(1, vec![block(0), tagged(1), block(2)]);
        queue.push_batch(2, vec![tagged(5), block(6)]);
        let order: Vec<u32> =
            std::iter::from_fn(|| queue.next(Block::plain).map(|b| b.id)).collect();
        assert_eq!(order, vec![6, 5, 0, 2, 1]);
    }

    #[test]
    fn preferred_segments_go_first_within_main_content_then_the_rest() {
        let mut queue = Queue::default();
        queue.push_batch(
            1,
            vec![in_main(tagged(0)), in_main(block(1)), tagged(2), block(3)],
        );
        let order: Vec<u32> =
            std::iter::from_fn(|| queue.next(Block::plain).map(|b| b.id)).collect();
        assert_eq!(order, vec![1, 0, 3, 2]);
    }

    #[test]
    fn a_segment_already_queued_is_not_queued_twice() {
        let mut queue = Queue::default();
        queue.push_batch(1, vec![block(0), block(1)]);
        assert_eq!(queue.next(|_| false).map(|b| b.id), Some(0));
        queue.push_batch(2, vec![block(0), block(1), block(2)]);
        assert_eq!(order(&mut queue), vec![2, 1]);
        assert!(queue.next(|_| false).is_none());
    }
}

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
}

impl Block {
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
    /// Segments that came into view.
    Blocks { gen: u64, blocks: Vec<Block> },
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
        PageMessage::Blocks { gen, mut blocks } => {
            if blocks.len() > MAX_BLOCKS {
                return None;
            }
            blocks.retain(Block::within_caps);
            Some(PageMessage::Blocks { gen, blocks })
        }
    }
}

/// Segments waiting for translation. The newest batch is what just came into
/// view; a segment is queued once per translation.
#[derive(Debug, Default)]
pub struct Queue {
    batches: VecDeque<VecDeque<Block>>,
    queued: HashSet<u32>,
}

impl Queue {
    pub fn push_batch(&mut self, blocks: Vec<Block>) {
        let fresh: VecDeque<Block> = blocks
            .into_iter()
            .filter(|block| self.queued.insert(block.id))
            .collect();
        if !fresh.is_empty() {
            self.batches.push_front(fresh);
        }
    }

    pub fn next(&mut self) -> Option<Block> {
        loop {
            let batch = self.batches.front_mut()?;
            if let Some(block) = batch.pop_front() {
                return Some(block);
            }
            self.batches.pop_front();
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
                r#"{"kind":"blocks","gen":3,"blocks":[{"id":0,"runs":[{"text":"詳しくは"},{"text":"こちら","tag":0}]}]}"#
            ),
            Some(PageMessage::Blocks {
                gen: 3,
                blocks: vec![Block {
                    id: 0,
                    runs: vec![run("詳しくは", None), run("こちら", Some(0))]
                }]
            })
        );
    }

    #[test]
    fn malformed_or_oversized_messages_are_dropped() {
        let long_lang = format!(r#"{{"kind":"start","gen":1,"lang":"{}"}}"#, "a".repeat(100));
        let many_blocks = format!(
            r#"{{"kind":"blocks","gen":1,"blocks":[{}]}}"#,
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
            r#"{{"kind":"blocks","gen":1,"blocks":[{{"id":0,"runs":[{many_runs}]}},{{"id":1,"runs":[{{"text":"{long_text}"}}]}},{{"id":99999,"runs":[{{"text":"x"}}]}},{{"id":2,"runs":[{{"text":"段落2"}}]}}]}}"#
        );
        assert_eq!(
            parse_message(&body),
            Some(PageMessage::Blocks {
                gen: 1,
                blocks: vec![block(2)]
            })
        );
    }

    #[test]
    fn the_newest_batch_goes_first_in_its_own_order() {
        let mut queue = Queue::default();
        queue.push_batch(vec![block(0), block(1)]);
        queue.push_batch(vec![block(5), block(6)]);
        let order: Vec<u32> = std::iter::from_fn(|| queue.next().map(|b| b.id)).collect();
        assert_eq!(order, vec![5, 6, 0, 1]);
    }

    #[test]
    fn a_segment_already_queued_is_not_queued_twice() {
        let mut queue = Queue::default();
        queue.push_batch(vec![block(0), block(1)]);
        assert_eq!(queue.next().map(|b| b.id), Some(0));
        queue.push_batch(vec![block(0), block(1), block(2)]);
        let order: Vec<u32> = std::iter::from_fn(|| queue.next().map(|b| b.id)).collect();
        assert_eq!(order, vec![2, 1]);
        assert!(queue.next().is_none());
    }
}

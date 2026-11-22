use crate::app::{
    editor::EditorApp,
    render::RenderedBlock,
    transport::{Scope, Transport},
};
use eframe::egui::{self, Color32, Pos2, Rect};
use std::ops::Range;

#[derive(Default)]
pub(crate) struct PlaybackGuide {
    blocks: Vec<TimedBlock>,
    last_followed: Option<u64>,
    pub(crate) word: Option<WordCursor>,
    pub(crate) reveal: Option<u64>,
}

struct TimedBlock {
    id: u64,
    time: Range<f64>,
    words: Vec<TimedWord>,
}

struct TimedWord {
    time: Range<f64>,
    cursor: WordCursor,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct WordKey {
    pub(super) sentence: u64,
    pub(super) token: usize,
}

#[derive(Clone, Copy, Default)]
pub(crate) struct PlaybackAnchor {
    pub(super) time: f64,
    word: Option<(u64, WordKey)>,
}

#[derive(Clone)]
pub(crate) struct WordCursor {
    pub(super) block: u64,
    pub(super) key: WordKey,
    pub(super) chars: Option<Range<usize>>,
}

impl PlaybackGuide {
    pub(crate) fn replace(&mut self, blocks: &[RenderedBlock]) {
        *self = Self::default();
        for block in blocks.iter().filter(|block| block.duration > 0.0) {
            let mut words = Vec::new();
            for sentence in &block.timing {
                let Some(state) = block.sentences.get(sentence.sentence.index) else {
                    continue;
                };
                for phones in sentence
                    .phones
                    .chunk_by(|a, b| a.reference[0] == b.reference[0])
                {
                    let first = &phones[0];
                    let last = &phones[phones.len() - 1];
                    let token = first.reference[0];
                    let Some(word) = state.words.get(token) else {
                        continue;
                    };
                    if matches!(word.pos, 0x2000 | 0x2001) {
                        continue;
                    }
                    let start = sentence.start + first.start;
                    let end = sentence.start + last.start + last.duration;
                    if start < end {
                        let offset = sentence.sentence.chars.start;
                        let chars = state
                            .word_positions
                            .ranges
                            .get(token)
                            .and_then(Option::as_ref)
                            .filter(|_| state.text == state.word_positions.source)
                            .map(|range| offset + range.start..offset + range.end);
                        words.push(TimedWord {
                            time: start..end,
                            cursor: WordCursor {
                                block: block.id,
                                key: WordKey {
                                    sentence: state.id,
                                    token,
                                },
                                chars,
                            },
                        });
                    }
                }
            }
            self.blocks.push(TimedBlock {
                id: block.id,
                time: block.start..block.start + block.duration,
                words,
            });
        }
    }

    fn block_at(&self, time: f64, scope: Scope) -> Option<&TimedBlock> {
        self.blocks
            .iter()
            .filter(|block| {
                block.time.contains(&time)
                    && match scope {
                        Scope::Project => true,
                        Scope::Block(id) => block.id == id,
                    }
            })
            .max_by(|a, b| a.time.start.total_cmp(&b.time.start))
    }

    fn at(&self, time: f64, scope: Scope) -> Option<(u64, Option<WordCursor>)> {
        let block = self.block_at(time, scope)?;
        let word = block.words.partition_point(|word| word.time.end <= time);
        let cursor = block
            .words
            .get(word)
            .filter(|word| word.time.contains(&time))
            .or_else(|| {
                (time == block.time.start)
                    .then(|| block.words.iter().find(|word| word.cursor.chars.is_some()))
                    .flatten()
            })
            .map(|word| word.cursor.clone());
        Some((block.id, cursor))
    }

    pub(super) fn pause_position(&self, time: f64, scope: Scope) -> Option<f64> {
        let block = self.block_at(time, scope).or_else(|| {
            let id = self.last_followed?;
            self.blocks.iter().find(|block| block.id == id)
        })?;
        block.words.iter().rev().find_map(|word| {
            (word.time.start <= time && word.cursor.chars.is_some()).then_some(word.time.start)
        })
    }

    pub(super) fn anchor(&self, time: f64, scope: Scope) -> PlaybackAnchor {
        let word = self.block_at(time, scope).and_then(|block| {
            block.words.iter().find_map(|word| {
                (word.cursor.chars.is_some() && (word.time.start - time).abs() < 0.001)
                    .then_some((block.id, word.cursor.key))
            })
        });
        PlaybackAnchor { time, word }
    }

    pub(super) fn retime(&self, anchor: PlaybackAnchor) -> f64 {
        anchor
            .word
            .and_then(|(block, key)| self.word_position(block, key))
            .unwrap_or(anchor.time)
    }

    pub(super) fn text_position(&self, id: u64, caret: usize) -> Option<f64> {
        let block = self.blocks.iter().find(|block| block.id == id)?;
        let mut last = None;
        for word in &block.words {
            if let Some(chars) = &word.cursor.chars {
                if caret < chars.end {
                    return Some(word.time.start);
                }
                last = Some(word.time.start);
            }
        }
        last
    }

    pub(super) fn word_position(&self, id: u64, key: WordKey) -> Option<f64> {
        self.blocks
            .iter()
            .find(|block| block.id == id)?
            .words
            .iter()
            .find(|word| word.cursor.key == key)
            .map(|word| word.time.start)
    }
}

impl EditorApp {
    pub(crate) fn follow_playback(&mut self) {
        let guide = &mut self.playback_guide;
        let current = guide.at(self.playhead, self.playback_scope);
        if self.transport == Transport::Playing && !self.playback_pending && self.player.loaded() {
            if let Some((id, _)) = &current
                && guide.last_followed != Some(*id)
            {
                self.selection.select_only(*id);
                guide.last_followed = Some(*id);
                guide.reveal = Some(*id);
            }
        } else {
            guide.last_followed = None;
        }
        guide.word = current
            .filter(|(id, _)| self.selection.current() == Some(*id))
            .and_then(|(_, cursor)| cursor);
    }
}

pub(super) fn word_bounds(
    galley: &egui::Galley,
    origin: Pos2,
    chars: Range<usize>,
) -> Option<Rect> {
    let mut bounds = Rect::NOTHING;
    word_rects(galley, origin, chars, |rect| bounds = bounds.union(rect));
    bounds.is_positive().then_some(bounds)
}

pub(crate) fn highlight(
    painter: &egui::Painter,
    output: &egui::text_edit::TextEditOutput,
    chars: Range<usize>,
    color: Color32,
) {
    let painter = painter.with_clip_rect(output.text_clip_rect);
    let fill = egui::Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 38);
    word_rects(&output.galley, output.galley_pos, chars, |rect| {
        painter.rect(
            rect,
            0,
            fill,
            egui::Stroke::new(1.0, color),
            egui::StrokeKind::Inside,
        );
    });
}

fn word_rects(
    galley: &egui::Galley,
    origin: Pos2,
    chars: Range<usize>,
    mut paint: impl FnMut(Rect),
) {
    let mut index = 0;
    for row in &galley.rows {
        let len = row.glyphs.len();
        let start = chars.start.saturating_sub(index);
        let end = chars.end.saturating_sub(index).min(len);
        if start < end {
            let origin = origin + row.pos.to_vec2();
            paint(Rect::from_min_max(
                origin + egui::vec2(row.x_offset(egui::text::CharIndex(start)), 0.0),
                origin + egui::vec2(row.x_offset(egui::text::CharIndex(end)), row.size.y),
            ));
        }
        index += len + usize::from(row.ends_with_newline);
        if index >= chars.end {
            break;
        }
    }
}

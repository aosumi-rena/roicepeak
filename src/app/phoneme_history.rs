use crate::app::{
    block_selection::BlockSelection, document::Project, edit_history::Entry, editor::EditorApp,
};
use eframe::egui::{self, Context, Id};

#[path = "phoneme_history_scope.rs"]
mod scope;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Lane {
    Accent,
    Intonation,
    Duration,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct Target {
    pub block: u64,
    pub sentence: u64,
    pub lane: Lane,
}

#[derive(Clone, Copy)]
struct Signal {
    frame: u64,
    key: Id,
    target: Target,
    ended: bool,
    ambiguous: bool,
}

#[derive(Clone, Copy)]
struct Frame {
    frame: u64,
    barrier: bool,
}

struct Pending {
    key: Id,
    target: Target,
    before: Entry,
}

#[derive(Default)]
pub(crate) struct State {
    pending: Option<Pending>,
}

#[derive(Clone)]
struct Retained<T: Clone> {
    frame: u64,
    started_frame: u64,
    value: T,
}

fn signal_id() -> Id {
    Id::new("phoneme-history-signal")
}
fn blocked_id() -> Id {
    Id::new("phoneme-history-blocked")
}
fn frame_id() -> Id {
    Id::new("phoneme-history-frame")
}
fn terminal_id() -> Id {
    Id::new("phoneme-history-terminal-frame")
}

pub(crate) fn begin_frame(ctx: &Context) {
    let frame = ctx.cumulative_frame_nr();
    let (down, barrier) = ctx.input(|input| {
        (
            input.pointer.primary_down(),
            !input.focused
                || input.events.iter().any(|event| {
                    matches!(
                        event,
                        egui::Event::Ime(_)
                            | egui::Event::Copy
                            | egui::Event::Cut
                            | egui::Event::Paste(_)
                    )
                }),
        )
    });
    ctx.data_mut(|data| {
        data.remove::<Signal>(signal_id());
        let terminal = data.get_temp::<u64>(terminal_id()) == Some(frame);
        data.insert_temp(
            frame_id(),
            Frame {
                frame,
                barrier: barrier || terminal,
            },
        );
        if !down {
            data.remove::<bool>(blocked_id());
        } else if barrier {
            data.insert_temp(blocked_id(), true);
        }
    });
}

pub(crate) fn allowed(ctx: &Context) -> bool {
    let frame = ctx.cumulative_frame_nr();
    ctx.data(|data| {
        data.get_temp::<u64>(terminal_id()) != Some(frame)
            && !data.get_temp::<bool>(blocked_id()).unwrap_or(false)
            && data
                .get_temp::<Frame>(frame_id())
                .is_some_and(|state| state.frame == frame && !state.barrier)
    })
}

pub(crate) fn pressed(ctx: &Context) -> bool {
    ctx.input(|input| {
        input.pointer.primary_pressed()
            || input.events.iter().any(|event| {
                matches!(
                    event,
                    egui::Event::PointerButton {
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        ..
                    }
                )
            })
    })
}

pub(crate) fn note(ctx: &Context, key: Id, target: Target, ended: bool) {
    if !allowed(ctx) {
        return;
    }
    let frame = ctx.cumulative_frame_nr();
    ctx.data_mut(|data| {
        let mut signal = data
            .get_temp::<Signal>(signal_id())
            .filter(|s| s.frame == frame)
            .unwrap_or(Signal {
                frame,
                key,
                target,
                ended: false,
                ambiguous: false,
            });
        signal.ambiguous |= signal.key != key || signal.target != target;
        signal.ended |= ended;
        data.insert_temp(signal_id(), signal);
    });
}

fn block(ctx: &Context) {
    let down = ctx.input(|input| input.pointer.primary_down());
    ctx.data_mut(|data| {
        data.remove::<Signal>(signal_id());
        data.insert_temp(blocked_id(), down);
    });
}

fn terminal(ctx: &Context) {
    let frame = ctx.cumulative_frame_nr();
    ctx.data_mut(|data| {
        data.insert_temp(terminal_id(), frame);
        data.remove::<Signal>(signal_id());
    });
}

pub(crate) fn load_capture<T: Clone + Send + Sync + 'static>(ctx: &Context, id: Id) -> Option<T> {
    let frame = ctx.cumulative_frame_nr();
    let retained = ctx.data(|data| data.get_temp::<Retained<T>>(id));
    if allowed(ctx) {
        if let Some(retained) =
            retained.filter(|r| r.frame == frame || r.frame.checked_add(1) == Some(frame))
        {
            return Some(retained.value);
        }
    }
    clear_capture::<T>(ctx, id);
    None
}

pub(crate) fn capture_started_this_frame<T: Clone + Send + Sync + 'static>(
    ctx: &Context,
    id: Id,
) -> bool {
    let frame = ctx.cumulative_frame_nr();
    ctx.data(|data| {
        data.get_temp::<Retained<T>>(id)
            .is_some_and(|retained| retained.started_frame == frame)
    })
}

pub(crate) fn store_capture<T: Clone + Send + Sync + 'static>(ctx: &Context, id: Id, value: T) {
    let frame = ctx.cumulative_frame_nr();
    ctx.data_mut(|data| {
        let started_frame = data
            .get_temp::<Retained<T>>(id)
            .filter(|retained| {
                retained.frame == frame || retained.frame.checked_add(1) == Some(frame)
            })
            .map_or(frame, |retained| retained.started_frame);
        data.insert_temp(
            id,
            Retained {
                frame,
                started_frame,
                value,
            },
        );
    });
}

pub(crate) fn clear_capture<T: Clone + Send + Sync + 'static>(ctx: &Context, id: Id) {
    ctx.data_mut(|data| data.remove::<Retained<T>>(id));
}

impl EditorApp {
    pub(crate) fn finish_phoneme_gesture(&mut self, ctx: &Context) {
        if let Some(pending) = self.phoneme_history.pending.take() {
            self.store_undo(pending.before);
            terminal(ctx);
        }
        block(ctx);
    }

    pub(crate) fn record_history_frame(
        &mut self,
        ctx: &Context,
        before: Project,
        selection: BlockSelection,
    ) {
        if self.history_navigation {
            self.phoneme_history = State::default();
            terminal(ctx);
            block(ctx);
            return;
        }
        let frame = ctx.cumulative_frame_nr();
        let signal = ctx
            .data(|data| data.get_temp::<Signal>(signal_id()))
            .filter(|signal| signal.frame == frame && !signal.ambiguous && allowed(ctx));
        let changed = !before.same_content(&self.project);
        let signal = signal.filter(|signal| {
            self.selected()
                .and_then(|i| self.project.blocks.get(i))
                .is_some_and(|b| b.id == signal.target.block)
                && selection.current() == self.selection.current()
                && selection.ids(&before) == self.selection.ids(&self.project)
                && self.reorder.is_none()
                && self.error.is_none()
                && self.pending.is_none()
                && !self.about
                && !self.preferences
                && !self.dictionary
                && !self.export.active()
                && !self.resource_settings
                && !self.experimental_settings
                && !self.shortcut_settings
                && self.voice_replacement.is_none()
                && !crate::app::emotion_presets::active(ctx)
                && match signal.target.lane {
                    Lane::Accent => self.mode == Some(crate::app::editor::Mode::Accent),
                    Lane::Intonation => self.mode == Some(crate::app::editor::Mode::Intonation),
                    Lane::Duration => self.mode == Some(crate::app::editor::Mode::Duration),
                }
                && (!changed || scope::only_lane(&before, &self.project, signal.target))
        });
        let Some(signal) = signal else {
            self.finish_phoneme_gesture(ctx);
            if changed {
                self.push_history(before, selection);
            }
            return;
        };
        if self
            .phoneme_history
            .pending
            .as_ref()
            .is_some_and(|p| p.key != signal.key || p.target != signal.target)
        {
            if let Some(pending) = self.phoneme_history.pending.take() {
                self.store_undo(pending.before);
            }
        }
        if changed {
            self.content_changed(&before);
        }
        if self.phoneme_history.pending.is_none() {
            self.phoneme_history.pending = Some(Pending {
                key: signal.key,
                target: signal.target,
                before: Entry {
                    project: before,
                    selection,
                },
            });
        }
        let down = ctx.input(|input| input.pointer.primary_down());
        if signal.ended || !down {
            if let Some(pending) = self.phoneme_history.pending.take() {
                self.store_undo(pending.before);
                terminal(ctx);
            }
        }
    }
}

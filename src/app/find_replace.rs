//! Literal UTF-8 search and replacement in project blocks.
use crate::app::style::{button_paint, text_button};
use crate::app::{document::Project, i18n, style};
use eframe::egui::{self, Color32, Rect, Response, Sense, Stroke, StrokeKind, Vec2};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Match {
    pub block: usize,
    pub start: usize,
    pub end: usize,
}

#[derive(Default)]
pub struct FindReplace {
    pub open: bool,
    pub find: String,
    pub replacement: String,
    pub current: Option<Match>,
    pub status: String,
    was_open: bool,
    focus_requested: bool,
    last_matches: Vec<Match>,
    search_keys: FieldKeys,
    replacement_keys: FieldKeys,
}

#[derive(Clone, Copy)]
enum Action {
    Next,
    Previous,
    Replace,
    ReplaceAll,
}

impl FindReplace {
    pub fn reveal(&mut self) {
        self.open = true;
        self.focus_requested = true;
    }

    fn refresh_matches(
        &mut self,
        matches: &[Match],
        selected: &mut Option<usize>,
        force: bool,
    ) -> Option<Match> {
        let previous = self.current;
        self.current = previous
            .filter(|item| matches.contains(item))
            .or_else(|| matches.first().copied());
        let list_changed = self.last_matches.as_slice() != matches;
        let changed = force || previous != self.current || list_changed;
        if list_changed {
            self.last_matches.clear();
            self.last_matches.extend_from_slice(matches);
        }
        if changed {
            if let Some(current) = self.current {
                *selected = Some(current.block);
            }
            self.current
        } else {
            None
        }
    }

    fn matches(&self, project: &Project) -> Result<Vec<Match>, String> {
        if self.find.is_empty() {
            return Err("Enter text to find".into());
        }
        if self.find.len() > 4096 || self.replacement.len() > 65_536 {
            return Err("Search or replacement is too long".into());
        }
        let mut bytes = 0usize;
        let mut found = Vec::new();
        for (block, item) in project.blocks.iter().enumerate() {
            bytes = bytes
                .checked_add(item.text.len())
                .ok_or("Search size overflow")?;
            if bytes > 16 * 1024 * 1024 {
                return Err("Search exceeds 16 MiB of text".into());
            }
            let mut search_from = 0;
            while let Some(offset) = item.text[search_from..].find(&self.find) {
                let start = search_from + offset;
                if found.len() >= 100_000 {
                    return Err("More than 100000 matches; narrow the search".into());
                }
                found.push(Match {
                    block,
                    start,
                    end: start + self.find.len(),
                });
                // Advance one scalar so matches that overlap remain navigable.
                search_from = start + item.text[start..].chars().next().unwrap().len_utf8();
            }
        }
        Ok(found)
    }

    fn act(
        &mut self,
        action: Action,
        project: &mut Project,
        selected: &mut Option<usize>,
    ) -> Result<(), String> {
        let found = self.matches(project)?;
        if found.is_empty() {
            self.current = None;
            self.status = "No matches".into();
            return Ok(());
        }
        if matches!(action, Action::ReplaceAll) {
            let replacement_matches = replacement_matches(&found, self.current);
            let mut replacement_bytes = 0usize;
            let mut edited = Vec::with_capacity(project.blocks.len());
            let mut matches = replacement_matches.iter().peekable();
            for (index, block) in project.blocks.iter().enumerate() {
                let mut text = String::new();
                let mut position = 0;
                while matches.peek().is_some_and(|m| m.block == index) {
                    let item = matches.next().unwrap();
                    let length = text.len() + item.start - position + self.replacement.len();
                    if length > 1_000_000 {
                        return Err("Replacement would exceed block text limit".into());
                    }
                    text.push_str(&block.text[position..item.start]);
                    text.push_str(&self.replacement);
                    position = item.end;
                }
                if text.len() + block.text.len() - position > 1_000_000 {
                    return Err("Replacement would exceed block text limit".into());
                }
                text.push_str(&block.text[position..]);
                replacement_bytes = replacement_bytes
                    .checked_add(text.len())
                    .ok_or("Replacement size overflow")?;
                if replacement_bytes > 16 * 1024 * 1024 {
                    return Err("Replacement would exceed 16 MiB of text".into());
                }
                edited.push(text);
            }
            let mut staged = Vec::new();
            for (index, (block, text)) in project.blocks.iter().zip(edited).enumerate() {
                if block.text != text {
                    let mut updated = block.clone();
                    updated.set_text(text)?;
                    staged.push((index, updated));
                }
            }
            for (index, block) in staged {
                project.blocks[index] = block;
            }
            self.status = format!("Replaced {} matches", replacement_matches.len());
            return Ok(());
        }
        if matches!(action, Action::Replace) {
            if let Some(current) = self.current.filter(|m| found.contains(m)) {
                let total = project
                    .blocks
                    .iter()
                    .try_fold(0usize, |sum, b| sum.checked_add(b.text.len()))
                    .ok_or("Replacement size overflow")?;
                if total - (current.end - current.start) + self.replacement.len() > 16 * 1024 * 1024
                {
                    return Err("Replacement would exceed 16 MiB of text".into());
                }
                let block = &mut project.blocks[current.block];
                let new_len =
                    block.text.len() - (current.end - current.start) + self.replacement.len();
                if new_len > 1_000_000 {
                    return Err("Replacement would exceed block text limit".into());
                }
                let mut text = block.text.clone();
                text.replace_range(current.start..current.end, &self.replacement);
                block.set_text(text)?;
                *selected = Some(current.block);
                self.status = "Replaced one match".into();
                return Ok(());
            }
        }
        let position = self
            .current
            .and_then(|current| found.iter().position(|m| *m == current));
        let index = if matches!(action, Action::Previous) {
            position.map_or(found.len() - 1, |i| (i + found.len() - 1) % found.len())
        } else {
            position.map_or_else(
                || {
                    found
                        .iter()
                        .position(|m| Some(m.block) >= *selected)
                        .unwrap_or(0)
                },
                |i| (i + 1) % found.len(),
            )
        };
        let item = found[index];
        self.current = Some(item);
        *selected = Some(item.block);
        self.status.clear();
        Ok(())
    }

    pub fn show(
        &mut self,
        ctx: &egui::Context,
        project: &mut Project,
        selected: &mut Option<usize>,
        locale: crate::app::i18n::Locale,
    ) -> Option<Match> {
        if !self.open {
            self.was_open = false;
            self.search_keys.composing = false;
            self.replacement_keys.composing = false;
            return None;
        }
        let request_focus = std::mem::take(&mut self.focus_requested) || !self.was_open;
        self.was_open = true;
        let mut close = false;
        let mut actions = Vec::new();
        let mut selection = None;
        let title = i18n::tr(locale, "検索と置換", "Find and replace");
        egui::Window::new(title)
            .id(egui::Id::new("find-replace"))
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .fixed_size([360.0, 284.0])
            .default_pos(egui::pos2(
                ctx.content_rect().left(),
                (ctx.content_rect().bottom() - 284.0).max(ctx.content_rect().top()),
            ))
            .frame(
                egui::Frame::NONE
                    .fill(Color32::WHITE)
                    .corner_radius(8)
                    .shadow(egui::epaint::Shadow {
                        offset: [2, 2],
                        blur: 8,
                        spread: 0,
                        color: Color32::from_black_alpha(48),
                    }),
            )
            .show(ctx, |ui| {
                ui.set_min_size(Vec2::new(360.0, 284.0));
                let origin = ui.max_rect().min;
                ui.painter().rect_stroke(
                    Rect::from_min_size(origin, Vec2::new(360.0, 284.0)),
                    8,
                    Stroke::new(1.0, Color32::from_gray(221)),
                    StrokeKind::Inside,
                );
                let at =
                    |x, y, w, h| Rect::from_min_size(origin + Vec2::new(x, y), Vec2::new(w, h));
                style::label(
                    ui,
                    origin + Vec2::new(20.0, 32.0),
                    title,
                    23.04 / 1.48,
                    Color32::from_rgba_unmultiplied(27, 43, 121, 204),
                );
                if icon_button(
                    ui,
                    at(308.0, 16.0, 32.0, 32.0),
                    14,
                    i18n::tr(locale, "閉じる", "Close"),
                    true,
                )
                .clicked()
                {
                    close = true;
                }
                for (y, text) in [
                    (74.0, i18n::tr(locale, "検索", "Find")),
                    (146.0, i18n::tr(locale, "置換", "Replace with")),
                ] {
                    style::label(
                        ui,
                        origin + Vec2::new(20.0, y),
                        text,
                        20.88 / 1.48,
                        Color32::from_gray(102),
                    );
                }
                if request_focus {
                    let id = ui.id().with("find-search");
                    if !ui.memory(|memory| memory.has_focus(id)) {
                        ui.memory_mut(|memory| memory.request_focus(id));
                    }
                }
                let (search, returns) = text_field(
                    ui,
                    at(20.0, 92.0, 320.0, 32.0),
                    "find-search",
                    &mut self.find,
                    "",
                    true,
                    &mut self.search_keys,
                );
                if search.changed() {
                    self.status.clear();
                }
                let _ = text_field(
                    ui,
                    at(20.0, 164.0, 320.0, 32.0),
                    "find-replacement",
                    &mut self.replacement,
                    "",
                    false,
                    &mut self.replacement_keys,
                );
                let found = if self.find.is_empty() {
                    Ok(Vec::new())
                } else {
                    self.matches(project)
                };
                let matches = found.as_ref().map_or(&[][..], |items| items.as_slice());
                selection =
                    self.refresh_matches(matches, selected, search.changed() || request_focus);
                let position = self
                    .current
                    .and_then(|current| matches.iter().position(|item| *item == current))
                    .map_or(0, |index| index + 1);
                let count = i18n::format(
                    locale,
                    "{} of {}",
                    &[&position.to_string(), &matches.len().to_string()],
                );
                let count_rect = at(288.0, 92.0, 44.0, 32.0);
                ui.painter().with_clip_rect(count_rect).text(
                    count_rect.right_center(),
                    egui::Align2::RIGHT_CENTER,
                    count,
                    style::font(20.88 / 1.48),
                    Color32::from_gray(128),
                );
                let detail = found
                    .as_ref()
                    .err()
                    .map_or(self.status.as_str(), String::as_str);
                if !detail.is_empty() {
                    ui.interact(count_rect, ui.id().with("find-count"), Sense::hover())
                        .on_hover_text(i18n::message(locale, detail).as_ref());
                }
                let can_find = !matches.is_empty();
                let can_replace = can_find && !self.replacement.is_empty();
                if can_find {
                    actions.extend(std::iter::repeat(Action::Next).take(returns));
                }
                if icon_button(
                    ui,
                    at(20.0, 228.0, 32.0, 32.0),
                    21,
                    i18n::tr(locale, "前を検索", "Previous"),
                    can_find,
                )
                .clicked()
                {
                    actions.push(Action::Previous);
                }
                if icon_button(
                    ui,
                    at(56.0, 228.0, 32.0, 32.0),
                    42,
                    i18n::tr(locale, "次を検索", "Next"),
                    can_find,
                )
                .clicked()
                {
                    actions.push(Action::Next);
                }
                if text_button(
                    ui,
                    at(136.0, 228.0, 100.0, 32.0),
                    i18n::tr(locale, "置換", "Replace"),
                    false,
                    can_replace,
                )
                .clicked()
                {
                    actions.push(Action::Replace);
                }
                if text_button(
                    ui,
                    at(240.0, 228.0, 100.0, 32.0),
                    i18n::tr(locale, "すべて置換", "Replace all"),
                    true,
                    can_replace,
                )
                .clicked()
                {
                    actions.push(Action::ReplaceAll);
                }
            });
        self.open = !close;
        let acted = !actions.is_empty();
        let mut edited = false;
        for action in actions {
            if let Err(error) = self.act(action, project, selected) {
                self.status = error;
                break;
            }
            edited |= matches!(action, Action::Replace | Action::ReplaceAll);
        }
        if edited {
            if let Ok(found) = self.matches(project) {
                self.refresh_matches(&found, selected, false);
            }
        }
        if acted { self.current } else { selection }
    }
}

// Match discovery includes overlaps; replacement requires disjoint ranges.
fn replacement_matches(found: &[Match], current: Option<Match>) -> Vec<Match> {
    let mut chosen: Vec<Match> = Vec::with_capacity(found.len());
    for item in found {
        if let Some(previous) = chosen.last_mut() {
            if previous.block == item.block && item.start < previous.end {
                // The current match takes priority over its preceding overlap.
                if Some(*item) == current && Some(*previous) != current {
                    *previous = *item;
                }
                continue;
            }
        }
        chosen.push(*item);
    }
    chosen
}

#[derive(Default)]
struct FieldKeys {
    composing: bool,
}

impl FieldKeys {
    fn observe_ime(&mut self, events: &[egui::Event]) -> bool {
        let mut observed = false;
        for event in events {
            if let egui::Event::Ime(event) = event {
                observed = true;
                match event {
                    egui::ImeEvent::Preedit { text, .. } => self.composing = !text.is_empty(),
                    egui::ImeEvent::Commit(_) => self.composing = false,
                    #[expect(deprecated)]
                    egui::ImeEvent::Disabled => self.composing = false,
                    _ => {}
                }
            }
        }
        observed
    }

    fn read(&mut self, ui: &egui::Ui, id: egui::Id) -> (usize, bool) {
        let escape = ui.input(|input| {
            input.events.iter().any(|event| {
                matches!(
                    event,
                    egui::Event::Key {
                        key: egui::Key::Escape,
                        pressed: true,
                        ..
                    }
                )
            })
        });
        if escape
            && ui.memory(|memory| memory.focused().is_none() && memory.had_focus_last_frame(id))
        {
            ui.memory_mut(|memory| memory.request_focus(id));
        }
        if !ui.memory(|memory| memory.has_focus(id)) {
            self.composing = false;
            return (0, false);
        }
        ui.input_mut(|input| {
            let was_composing = self.composing;
            let ime_event = self.observe_ime(&input.events);
            if was_composing || self.composing || ime_event {
                return (0, false);
            }
            let mut returns = 0;
            let mut escape = false;
            input.events.retain(|event| {
                if let egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } = event
                {
                    if *key == egui::Key::Enter && !modifiers.any() {
                        returns += 1;
                        return false;
                    }
                    if *key == egui::Key::Escape {
                        escape = true;
                        return false;
                    }
                }
                true
            });
            (returns, escape)
        })
    }
}

fn text_field(
    ui: &mut egui::Ui,
    rect: Rect,
    id: &str,
    text: &mut String,
    hint: &str,
    count: bool,
    keys: &mut FieldKeys,
) -> (Response, usize) {
    let id = ui.id().with(id);
    let (returns, escape) = keys.read(ui, id);
    if escape {
        if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), id) {
            if let Some(range) = state.cursor.char_range() {
                state
                    .cursor
                    .set_char_range(Some(egui::text::CCursorRange::one(range.primary)));
                state.store(ui.ctx(), id);
            }
        }
    }
    let response = ui.put(
        rect,
        egui::TextEdit::singleline(text)
            .id(id)
            .return_key(None)
            .event_filter(egui::EventFilter {
                horizontal_arrows: true,
                vertical_arrows: true,
                escape: true,
                ..Default::default()
            })
            .font(style::font(23.04 / 1.48))
            .text_color(Color32::from_gray(102))
            .hint_text(egui::RichText::new(hint).color(Color32::from_gray(211)))
            .frame(egui::Frame::NONE)
            .margin(egui::Margin {
                left: 16,
                right: if count { 60 } else { 16 },
                top: 0,
                bottom: 0,
            })
            .vertical_align(egui::Align::Center),
    );
    if response.has_focus() {
        ui.input(|input| keys.observe_ime(&input.events));
    }
    let border = if response.has_focus() {
        125
    } else if response.hovered() {
        175
    } else {
        221
    };
    ui.painter().rect_stroke(
        rect,
        4,
        Stroke::new(
            if response.has_focus() { 2.0 } else { 1.0 },
            Color32::from_gray(border),
        ),
        StrokeKind::Inside,
    );
    (response, returns)
}

fn icon_button(
    ui: &mut egui::Ui,
    rect: Rect,
    number: u8,
    tooltip: &str,
    enabled: bool,
) -> Response {
    ui.add_enabled_ui(enabled, |ui| {
        let response = ui
            .interact(
                rect,
                ui.id().with(("find-icon", number)),
                if enabled {
                    Sense::click()
                } else {
                    Sense::hover()
                },
            )
            .on_hover_text(tooltip);
        response
            .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, tooltip));
        if number != 14 {
            button_paint(ui, rect, &response, false, enabled);
        }
        crate::app::native_icons::paint_dialog(ui, rect, number);
        response
    })
    .inner
}

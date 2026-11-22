use egui::containers::scroll_area::ScrollBarVisibility;
use egui::{
    Align, Button, Color32, CornerRadius, Frame, Id, IntoAtoms, Label, Layout, Margin, Rect,
    Response, RichText, ScrollArea, Sense, Separator, Stroke, TextStyle, Ui, UiBuilder, Vec2, pos2,
    vec2,
};

use super::icons;
use super::theme::{self, State, radius, size, space, text};
use crate::fl;
use crate::model::{DIVERGENCE_LIMIT, Upstream};

#[derive(Clone, Copy)]
pub enum Tone {
    Head,
    Branch,
    Remote,
    Tag,
}

#[derive(Clone, Copy)]
pub enum Kind {
    Primary,
    Danger,
    Secondary,
    Ghost,
}

pub struct Tab {
    pub icon: Option<&'static str>,
    pub label: String,
    pub hint: Option<String>,
    pub selected: bool,
    pub closable: bool,
    pub pinned: bool,
}

pub enum TabAction {
    Select(usize),
    Close(usize),
}

#[derive(Default)]
pub struct Row<'a> {
    pub depth: usize,
    pub icon: &'a str,
    pub tint: Option<Color32>,
    pub label: &'a str,
    pub detail: Option<&'a str>,
    pub active: bool,
    pub selected: bool,
    pub open: Option<bool>,
}

pub fn canvas(ui: &Ui) -> Frame {
    Frame::new().fill(theme::palette(ui.visuals()).canvas)
}

pub fn top_bar(ui: &Ui) -> Frame {
    canvas(ui).inner_margin(Margin::symmetric(space::M as i8, 0))
}

pub fn workspace(ui: &Ui, sidebar: bool) -> Frame {
    canvas(ui).inner_margin(Margin {
        left: if sidebar { 0 } else { space::M as i8 },
        right: space::M as i8,
        top: 0,
        bottom: space::M as i8,
    })
}

pub fn card<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    let p = theme::palette(ui.visuals());
    surface(ui, p.panel, p.canvas, radius::L, |ui| {
        ui.set_min_size(ui.available_size());
        add(ui)
    })
}

fn surface<R>(
    ui: &mut Ui,
    fill: Color32,
    outside: Color32,
    radius: u8,
    add: impl FnOnce(&mut Ui) -> R,
) -> R {
    let shown = Frame::new()
        .fill(fill)
        .corner_radius(CornerRadius::same(radius))
        .inner_margin(Margin::same(1))
        .show(ui, add);
    clip_corners(ui, shown.response.rect, radius, outside);
    shown.inner
}

fn clip_corners(ui: &Ui, rect: Rect, radius: u8, outside: Color32) {
    use std::f32::consts::{FRAC_PI_2, PI};
    const STEPS: u32 = 8;
    let r = f32::from(radius);
    let mut mesh = egui::Mesh::default();
    for (corner, center, start) in [
        (rect.left_top(), rect.left_top() + vec2(r, r), PI),
        (
            rect.right_top(),
            rect.right_top() + vec2(-r, r),
            PI + FRAC_PI_2,
        ),
        (rect.right_bottom(), rect.right_bottom() - vec2(r, r), 0.0),
        (
            rect.left_bottom(),
            rect.left_bottom() + vec2(r, -r),
            FRAC_PI_2,
        ),
    ] {
        let base = mesh.vertices.len() as u32;
        mesh.colored_vertex(corner, outside);
        for i in 0..=STEPS {
            let angle = start + FRAC_PI_2 * i as f32 / STEPS as f32;
            mesh.colored_vertex(center + r * vec2(angle.cos(), angle.sin()), outside);
        }
        for i in 0..STEPS {
            mesh.add_triangle(base, base + 1 + i, base + 2 + i);
        }
    }
    let painter = ui.painter();
    painter.add(egui::Shape::mesh(mesh));
    painter.rect_stroke(
        rect,
        CornerRadius::same(radius),
        Stroke::new(1.0, theme::palette(ui.visuals()).border),
        egui::StrokeKind::Inside,
    );
}

pub fn list_area<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    Frame::new()
        .inner_margin(Margin {
            left: space::S as i8,
            right: space::S as i8,
            top: space::S as i8,
            bottom: 0,
        })
        .show(ui, add)
        .inner
}

pub fn plain() -> Frame {
    Frame::new()
}

pub fn gutter() -> Frame {
    Frame::new().inner_margin(Margin {
        left: space::M as i8,
        ..Margin::ZERO
    })
}

pub fn body<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    Frame::new()
        .inner_margin(Margin::symmetric(space::L as i8, space::M as i8))
        .show(ui, add)
        .inner
}

pub fn bar_row<R>(ui: &mut Ui, height: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
    let width = ui.available_width();
    ui.allocate_ui_with_layout(
        vec2(width, height),
        Layout::left_to_right(Align::Center),
        |ui| {
            ui.set_min_size(vec2(width, height));
            add(ui)
        },
    )
    .inner
}

pub fn pane_header<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        let inner = Frame::new()
            .inner_margin(Margin::symmetric(space::S as i8, 0))
            .show(ui, |ui| bar_row(ui, size::HEADER, add))
            .inner;
        divider(ui);
        inner
    })
    .inner
}

pub fn heading(ui: &Ui, text: impl Into<String>) -> RichText {
    RichText::new(text)
        .strong()
        .color(theme::palette(ui.visuals()).text_muted)
}

pub fn dialog_title(ui: &Ui, text: impl Into<String>) -> RichText {
    RichText::new(text)
        .strong()
        .color(theme::palette(ui.visuals()).text_strong)
}

pub fn caption(ui: &Ui, text: impl Into<String>) -> RichText {
    RichText::new(text)
        .small()
        .color(theme::palette(ui.visuals()).text_muted)
}

fn overlay<R>(ui: &mut Ui, rect: Rect, layout: Layout, add: impl FnOnce(&mut Ui) -> R) -> R {
    add(&mut ui.new_child(UiBuilder::new().max_rect(rect).layout(layout)))
}

fn item_background(ui: &mut Ui, rect: Rect, state: State) -> Color32 {
    let (fill, color) = theme::palette(ui.visuals()).item(state);
    overlay(ui, rect, Layout::default(), |ui| {
        Frame::new()
            .fill(fill)
            .corner_radius(CornerRadius::same(radius::M))
            .show(ui, |ui| ui.set_min_size(rect.size()));
    });
    color
}

fn state(selected: bool, hovered: bool) -> State {
    if selected {
        State::Selected
    } else if hovered {
        State::Hover
    } else {
        State::Idle
    }
}

pub fn tabs(ui: &mut Ui, tabs: &[Tab]) -> Option<TabAction> {
    if tabs.is_empty() {
        return None;
    }

    let font = TextStyle::Body.resolve(ui.style());
    let measure = |text: &str| {
        ui.painter()
            .layout_no_wrap(text.to_owned(), font.clone(), Color32::TRANSPARENT)
            .size()
            .x
    };
    let measured: Vec<(f32, f32)> = tabs
        .iter()
        .map(|tab| {
            let icon = tab.icon.map_or(0.0, |icon| measure(icon) + space::S);
            (measure(&tab.label), icon)
        })
        .collect();
    let natural: Vec<f32> = tabs
        .iter()
        .zip(&measured)
        .map(|(tab, (text, icon))| {
            let close = if tab.closable {
                size::GLYPH + space::S
            } else {
                0.0
            };
            let min = if tab.closable {
                size::TAB_MIN_CLOSE
            } else {
                size::TAB_MIN
            };
            (text + icon + close + 2.0 * space::M).clamp(min, size::TAB_MAX)
        })
        .collect();

    let pinned = tabs.iter().take_while(|tab| tab.pinned).count();
    let pinned_width =
        natural[..pinned].iter().sum::<f32>() + space::XS * pinned.saturating_sub(1) as f32;
    let separator = if pinned > 0 && pinned < tabs.len() {
        ui.spacing().item_spacing.x
    } else {
        0.0
    };
    let unpinned = tabs.len() - pinned;
    let gaps = space::XS * unpinned.saturating_sub(1) as f32;
    let budget = (ui.available_width() - pinned_width - separator - gaps).max(0.0);
    let total: f32 = natural[pinned..].iter().sum();
    let mut widths = natural;
    if pinned < tabs.len() && total > budget && budget > 0.0 {
        for (i, tab) in tabs.iter().enumerate().skip(pinned) {
            let min = if tab.closable {
                size::TAB_MIN_CLOSE
            } else {
                size::TAB_MIN
            };
            widths[i] = (widths[i] * budget / total).max(min);
        }
    }

    let selected = tabs
        .iter()
        .position(|tab| tab.selected)
        .unwrap_or(usize::MAX);
    let memory = ui.scope_id().with("tabs-selected");
    let last = ui
        .ctx()
        .data_mut(|data| data.get_temp::<(usize, usize)>(memory));
    let reveal = last != Some((selected, tabs.len()));
    ui.ctx()
        .data_mut(|data| data.insert_temp(memory, (selected, tabs.len())));

    let mut action = None;
    if pinned > 0 {
        tab_row(ui, tabs, &measured, &widths, 0..pinned, false, &mut action);
    }
    if pinned < tabs.len() {
        ScrollArea::horizontal()
            .id_salt("tabs")
            .scroll_bar_visibility(ScrollBarVisibility::AlwaysHidden)
            .auto_shrink([true, true])
            .min_scrolled_width(1.0)
            .show(ui, |ui| {
                tab_row(
                    ui,
                    tabs,
                    &measured,
                    &widths,
                    pinned..tabs.len(),
                    reveal,
                    &mut action,
                );
            });
    }
    action
}

fn tab_row(
    ui: &mut Ui,
    tabs: &[Tab],
    measured: &[(f32, f32)],
    widths: &[f32],
    range: std::ops::Range<usize>,
    reveal: bool,
    action: &mut Option<TabAction>,
) {
    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = space::XS;
        for i in range {
            let tab = &tabs[i];
            let (text_width, icon_width) = measured[i];
            let close_space = if tab.closable {
                size::GLYPH + space::S
            } else {
                0.0
            };
            let (rect, response) =
                ui.allocate_exact_size(vec2(widths[i], size::CONTROL), Sense::click());
            if reveal && tab.selected {
                response.scroll_to_me(None);
            }
            let hovered = response.hovered();
            let color = item_background(ui, rect, state(tab.selected, hovered));
            let inner = rect.shrink2(vec2(space::M, 0.0));
            let mut label_rect = inner;
            label_rect.set_right(inner.right() - close_space);

            if tab.closable {
                if let Some(icon) = tab.icon {
                    let slot = Rect::from_min_size(
                        label_rect.min,
                        vec2((icon_width - space::S).max(0.0), label_rect.height()),
                    );
                    tab_icon(ui, slot, icon, color);
                    label_rect.min.x = (label_rect.min.x + icon_width).min(label_rect.right());
                }
                overlay(ui, label_rect, Layout::left_to_right(Align::Center), |ui| {
                    tab_label(ui, tab, color);
                });
            } else if let Some(icon) = tab.icon {
                let group = (icon_width + text_width).min(label_rect.width());
                let group_rect = Rect::from_center_size(
                    label_rect.center(),
                    vec2(group.max(0.0), label_rect.height()),
                );
                let slot = Rect::from_min_size(
                    group_rect.min,
                    vec2((icon_width - space::S).max(0.0), group_rect.height()),
                );
                tab_icon(ui, slot, icon, color);
                let mut text_rect = group_rect;
                text_rect.min.x = (text_rect.min.x + icon_width).min(text_rect.right());
                overlay(ui, text_rect, Layout::left_to_right(Align::Center), |ui| {
                    tab_label(ui, tab, color);
                });
            } else {
                overlay(
                    ui,
                    label_rect,
                    Layout::centered_and_justified(egui::Direction::LeftToRight),
                    |ui| tab_label(ui, tab, color),
                );
            }

            let close_rect = Rect::from_center_size(
                pos2(inner.right() - size::GLYPH / 2.0, rect.center().y),
                Vec2::splat(size::GLYPH),
            );
            if tab.closable
                && (tab.selected || hovered || ui.rect_contains_pointer(close_rect))
                && close(ui, close_rect, ui.scope_id().with(("close", i))).clicked()
            {
                *action = Some(TabAction::Close(i));
            }
            let response = match &tab.hint {
                Some(hint) => response.on_hover_text(hint),
                None => response,
            };
            if response.clicked() && action.is_none() {
                *action = Some(TabAction::Select(i));
            }
        }
    });
}

fn tab_icon(ui: &mut Ui, rect: Rect, glyph: &str, color: Color32) {
    overlay(
        ui,
        rect,
        Layout::centered_and_justified(egui::Direction::LeftToRight),
        |ui| {
            ui.add(Label::new(RichText::new(glyph).color(color)).selectable(false));
        },
    );
}

fn tab_label(ui: &mut Ui, tab: &Tab, color: Color32) {
    ui.add(
        Label::new(RichText::new(&tab.label).color(color))
            .truncate()
            .show_tooltip_when_elided(tab.hint.is_none())
            .selectable(false),
    );
}

fn close(ui: &mut Ui, rect: Rect, id: Id) -> Response {
    let response = ui.interact(rect, id, Sense::click());
    let p = theme::palette(ui.visuals());
    let color = if response.hovered() {
        p.text_strong
    } else {
        p.text_faint
    };
    overlay(
        ui,
        rect,
        Layout::centered_and_justified(egui::Direction::LeftToRight),
        |ui| {
            ui.add(Label::new(RichText::new(icons::X).small().color(color)).selectable(false));
        },
    );
    response
}

pub fn nav_row(ui: &mut Ui, row: Row<'_>, trailing: impl FnOnce(&mut Ui)) -> Response {
    let p = theme::palette(ui.visuals());
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), size::CONTROL), Sense::click());
    let color = item_background(ui, rect, state(row.selected, response.hovered()));
    let indent = space::M + row.depth as f32 * space::XL;
    let inner = Rect::from_min_max(
        pos2(rect.left() + indent, rect.top()),
        pos2(rect.right() - space::M, rect.bottom()),
    );
    overlay(ui, inner, Layout::right_to_left(Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = space::S + space::XS;
        trailing(ui);
        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
            disclosure(ui, row.open);
            let icon_color = match (row.tint, row.active) {
                (Some(tint), _) => tint,
                (None, true) => p.accent_text,
                (None, false) => p.text_faint,
            };
            ui.add(Label::new(RichText::new(row.icon).color(icon_color)).selectable(false));
            let text = RichText::new(row.label).color(color);
            let text = if row.active { text.strong() } else { text };
            label_with_detail(ui, text, row.detail);
        });
    });
    response
}

fn label_with_detail(ui: &mut Ui, name: RichText, detail: Option<&str>) {
    let Some(detail) = detail else {
        ui.add(Label::new(name).truncate().selectable(false));
        return;
    };
    let available = ui.available_width().max(0.0);
    let spacing = ui.spacing().item_spacing.x;
    let detail_width = text_width(ui, detail, TextStyle::Body).min(available * 0.55);
    let name_max = (available - detail_width - spacing).max(available * 0.35);
    ui.scope(|ui| {
        ui.set_max_width(name_max);
        ui.add(Label::new(name).truncate().selectable(false));
    });
    let detail = RichText::new(detail).color(theme::palette(ui.visuals()).text_muted);
    ui.add(Label::new(detail).truncate().selectable(false));
}

fn text_width(ui: &Ui, text: &str, style: TextStyle) -> f32 {
    ui.painter()
        .layout_no_wrap(
            text.to_owned(),
            style.resolve(ui.style()),
            Color32::TRANSPARENT,
        )
        .size()
        .x
}

pub fn tree<'a, T: Copy>(
    ui: &mut Ui,
    parent: Id,
    items: Vec<(&'a str, T)>,
    depth: usize,
    leaf: &mut impl FnMut(&mut Ui, usize, &'a str, T),
) {
    let mut groups: std::collections::BTreeMap<&str, Vec<(&str, T)>> = Default::default();
    let mut leaves = Vec::new();
    for (rest, item) in items {
        match rest.split_once('/') {
            Some((group, tail)) => groups.entry(group).or_default().push((tail, item)),
            None => leaves.push((rest, item)),
        }
    }
    for (group, children) in groups {
        let id = parent.with(group);
        let mut state =
            egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, true);
        let row = Row {
            depth,
            icon: icons::FOLDER_OPEN,
            label: group,
            open: Some(state.is_open()),
            ..Row::default()
        };
        if nav_row(ui, row, |_| {}).clicked() {
            state.toggle(ui);
        }
        let open = state.is_open();
        state.store(ui.ctx());
        if open {
            tree(ui, id, children, depth + 1, leaf);
        }
    }
    for (label, item) in leaves {
        leaf(ui, depth, label, item);
    }
}

pub enum TreeRow<T> {
    Heading {
        id: Id,
        title: String,
        count: usize,
        open: bool,
    },
    Folder {
        id: Id,
        depth: usize,
        name: String,
        open: bool,
    },
    Leaf {
        depth: usize,
        name: String,
        item: T,
    },
}

impl<T> TreeRow<T> {
    pub fn height(&self) -> f32 {
        match self {
            Self::Heading { .. } => size::CONTROL + space::M,
            Self::Folder { .. } | Self::Leaf { .. } => size::CONTROL,
        }
    }
}

pub fn flatten<T>(
    ctx: &egui::Context,
    id: Id,
    title: &str,
    items: Vec<(&str, T)>,
    tree: bool,
    out: &mut Vec<TreeRow<T>>,
) {
    let open =
        egui::collapsing_header::CollapsingState::load_with_default_open(ctx, id, true).is_open();
    out.push(TreeRow::Heading {
        id,
        title: title.to_owned(),
        count: items.len(),
        open,
    });
    if !open {
        return;
    }
    if tree {
        flatten_tree(ctx, id, items, 0, out);
    } else {
        for (path, item) in items {
            let name = path.rsplit('/').next().unwrap_or(path);
            out.push(TreeRow::Leaf {
                depth: 0,
                name: name.to_owned(),
                item,
            });
        }
    }
}

fn flatten_tree<T>(
    ctx: &egui::Context,
    parent: Id,
    items: Vec<(&str, T)>,
    depth: usize,
    out: &mut Vec<TreeRow<T>>,
) {
    let mut groups: std::collections::BTreeMap<&str, Vec<(&str, T)>> = Default::default();
    let mut leaves = Vec::new();
    for (rest, item) in items {
        match rest.split_once('/') {
            Some((group, tail)) => groups.entry(group).or_default().push((tail, item)),
            None => leaves.push((rest, item)),
        }
    }
    for (group, children) in groups {
        let id = parent.with(group);
        let open = egui::collapsing_header::CollapsingState::load_with_default_open(ctx, id, true)
            .is_open();
        out.push(TreeRow::Folder {
            id,
            depth,
            name: group.to_owned(),
            open,
        });
        if open {
            flatten_tree(ctx, id, children, depth + 1, out);
        }
    }
    for (name, item) in leaves {
        out.push(TreeRow::Leaf {
            depth,
            name: name.to_owned(),
            item,
        });
    }
}

pub fn row_tops<T>(rows: &[TreeRow<T>]) -> Vec<f32> {
    let mut tops = Vec::with_capacity(rows.len() + 1);
    let mut y = 0.0;
    for row in rows {
        tops.push(y);
        y += row.height();
    }
    tops.push(y);
    tops
}

pub fn virtual_rows(
    ui: &mut Ui,
    tops: &[f32],
    viewport: Rect,
    mut row: impl FnMut(&mut Ui, usize),
) {
    ui.set_height(tops.last().copied().unwrap_or(0.0));
    let scope = ui.scope_id();
    let x = ui.max_rect().x_range();
    let top = ui.max_rect().top();
    let first = tops
        .partition_point(|&t| t <= viewport.min.y)
        .saturating_sub(1);
    let last = tops
        .partition_point(|&t| t < viewport.max.y)
        .min(tops.len().saturating_sub(1));
    for i in first..last {
        let rect = Rect::from_x_y_ranges(x, top + tops[i]..=top + tops[i + 1]);
        ui.scope_builder(
            UiBuilder::new().max_rect(rect).scope_id(scope.with(i)),
            |ui| row(ui, i),
        );
    }
}

pub fn group(
    ui: &mut Ui,
    id: Id,
    title: &str,
    count: usize,
    default_open: bool,
    trailing: impl FnOnce(&mut Ui),
) -> bool {
    let mut state = egui::collapsing_header::CollapsingState::load_with_default_open(
        ui.ctx(),
        id,
        default_open,
    );
    if group_heading(ui, title, count, state.is_open(), trailing).clicked() {
        state.toggle(ui);
    }
    let open = state.is_open();
    state.store(ui.ctx());
    open
}

pub fn group_heading(
    ui: &mut Ui,
    title: &str,
    count: usize,
    open: bool,
    trailing: impl FnOnce(&mut Ui),
) -> Response {
    ui.add_space(space::M);
    nav_heading(ui, title, count, open, trailing)
}

fn nav_heading(
    ui: &mut Ui,
    title: &str,
    count: usize,
    open: bool,
    trailing: impl FnOnce(&mut Ui),
) -> Response {
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), size::CONTROL), Sense::click());
    item_background(ui, rect, state(false, response.hovered()));
    overlay(
        ui,
        rect.shrink2(vec2(space::M, 0.0)),
        Layout::left_to_right(Align::Center),
        |ui| {
            ui.spacing_mut().item_spacing.x = space::S + space::XS;
            disclosure(ui, Some(open));
            ui.add(Label::new(heading(ui, title)).selectable(false));
            number(ui, count);
            ui.with_layout(Layout::right_to_left(Align::Center), trailing);
        },
    );
    response
}

pub fn disclosure(ui: &mut Ui, open: Option<bool>) {
    let chevron = match open {
        Some(true) => icons::CHEVRON_DOWN,
        Some(false) => icons::CHEVRON_RIGHT,
        None => "",
    };
    let color = theme::palette(ui.visuals()).text_faint;
    ui.add_sized(
        vec2(text::CAPTION, size::CONTROL),
        Label::new(RichText::new(chevron).small().color(color)).selectable(false),
    );
}

#[expect(clippy::disallowed_methods)]
pub fn search(ui: &mut Ui, value: &mut String, hint: impl Into<String>) -> Response {
    let glyph = space::M + text::BODY + space::S;
    let response = ui.add(
        egui::TextEdit::singleline(value)
            .hint_text(hint.into())
            .return_key(None)
            .id_salt("search")
            .margin(Margin {
                left: glyph as i8,
                right: space::M as i8,
                top: (space::S + 1.0) as i8,
                bottom: (space::S + 1.0) as i8,
            })
            .min_size(vec2(0.0, size::CONTROL))
            .desired_width(f32::INFINITY),
    );
    ui.painter().text(
        pos2(response.rect.left() + space::M, response.rect.center().y),
        egui::Align2::LEFT_CENTER,
        icons::SEARCH,
        egui::FontId::proportional(text::BODY),
        theme::palette(ui.visuals()).text_faint,
    );
    response
}

pub fn button(ui: &mut Ui, kind: Kind, icon: &str, label: impl Into<String>) -> Response {
    button_sized(ui, kind, icon, label, 0.0)
}

pub fn button_truncated(
    ui: &mut Ui,
    kind: Kind,
    icon: &str,
    label: impl Into<String>,
    max_width: f32,
) -> Response {
    ui.scope(|ui| {
        ui.set_max_width(max_width);
        button_text(ui, kind, format!("{icon} {}", label.into()), 0.0, true)
    })
    .inner
}

pub fn button_sized(
    ui: &mut Ui,
    kind: Kind,
    icon: &str,
    label: impl Into<String>,
    width: f32,
) -> Response {
    button_text(ui, kind, format!("{icon} {}", label.into()), width, false)
}

pub fn text_button(ui: &mut Ui, kind: Kind, label: impl Into<String>) -> Response {
    text_button_sized(ui, kind, label, 0.0)
}

pub fn text_button_sized(
    ui: &mut Ui,
    kind: Kind,
    label: impl Into<String>,
    width: f32,
) -> Response {
    button_text(ui, kind, label.into(), width, false)
}

fn button_text(ui: &mut Ui, kind: Kind, text: String, width: f32, truncate: bool) -> Response {
    let p = theme::palette(ui.visuals());
    let enabled = ui.is_enabled();
    let text = RichText::new(text);
    let button = |text| {
        let button = Button::new(text).min_size(vec2(width, size::CONTROL));
        if truncate { button.truncate() } else { button }
    };
    ui.add(match kind {
        Kind::Primary if enabled => button(text.strong().color(p.on_accent))
            .fill(p.accent)
            .stroke(Stroke::NONE),
        Kind::Primary => button(text.color(p.text_faint))
            .fill(p.control)
            .stroke(Stroke::NONE),
        Kind::Danger if enabled => button(text.strong().color(p.on_danger))
            .fill(p.danger_fill)
            .stroke(Stroke::NONE),
        Kind::Danger => button(text.color(p.text_faint))
            .fill(p.control)
            .stroke(Stroke::NONE),
        Kind::Secondary => button(text),
        Kind::Ghost => button(text).frame_when_inactive(false).stroke(Stroke::NONE),
    })
}

fn ghost<'a>(text: impl IntoAtoms<'a>, square: bool) -> Button<'a> {
    let min = if square {
        Vec2::splat(size::CONTROL)
    } else {
        vec2(0.0, size::CONTROL)
    };
    Button::new(text)
        .frame_when_inactive(false)
        .stroke(Stroke::NONE)
        .min_size(min)
}

pub fn icon(ui: &mut Ui, glyph: &str, tooltip: impl Into<String>) -> Response {
    ui.add(ghost(glyph, true)).on_hover_text(tooltip.into())
}

fn menu_body(ui: &mut Ui, width: f32, add: impl FnOnce(&mut Ui)) {
    ui.set_width(width);
    ui.spacing_mut().item_spacing.y = 0.0;
    add(ui);
}

#[expect(clippy::disallowed_methods)]
fn menu_button(
    ui: &mut Ui,
    glyph: &str,
    tooltip: impl Into<String>,
    close: egui::PopupCloseBehavior,
    width: f32,
    add: impl FnOnce(&mut Ui),
) -> Response {
    let config = egui::containers::menu::MenuConfig::new().close_behavior(close);
    egui::containers::menu::MenuButton::from_button(ghost(glyph, true))
        .config(config)
        .ui(ui, |ui| menu_body(ui, width, add))
        .0
        .on_hover_text(tooltip.into())
}

pub fn menu(
    ui: &mut Ui,
    glyph: &str,
    tooltip: impl Into<String>,
    add: impl FnOnce(&mut Ui),
) -> Response {
    menu_button(
        ui,
        glyph,
        tooltip,
        egui::PopupCloseBehavior::CloseOnClick,
        size::MENU,
        add,
    )
}

pub fn popover(
    ui: &mut Ui,
    glyph: &str,
    tooltip: impl Into<String>,
    width: f32,
    add: impl FnOnce(&mut Ui),
) -> Response {
    menu_button(
        ui,
        glyph,
        tooltip,
        egui::PopupCloseBehavior::CloseOnClickOutside,
        width,
        add,
    )
}

pub fn counter(ui: &mut Ui, glyph: &str, tint: Color32, count: usize, tooltip: impl Into<String>) {
    let p = theme::palette(ui.visuals());
    let (tint, color) = if count == 0 {
        (p.text_faint, p.text_faint)
    } else {
        (tint, p.text_muted)
    };
    let glyph = RichText::new(glyph).color(tint);
    let number = RichText::new(count.to_string()).monospace().color(color);
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.x = space::XS;
        let parts = if ui.layout().prefer_right_to_left() {
            [number, glyph]
        } else {
            [glyph, number]
        };
        for part in parts {
            ui.add(Label::new(part).selectable(false));
        }
    })
    .response
    .on_hover_text(tooltip.into());
}

#[expect(clippy::disallowed_methods)]
pub fn context_menu(response: &Response, add: impl FnOnce(&mut Ui)) {
    response.context_menu(|ui| menu_body(ui, size::MENU, add));
}

pub fn menu_item(ui: &mut Ui, label: impl Into<String>) -> Response {
    let label = label.into();
    list_row(ui, false, |ui| {
        ui.add(Label::new(label).truncate().selectable(false));
    })
}

pub fn menu_heading(ui: &mut Ui, text: impl Into<String>) {
    let title = heading(ui, text);
    ui.allocate_ui_with_layout(
        vec2(ui.available_width(), size::CONTROL),
        Layout::left_to_right(Align::Center),
        |ui| {
            ui.add_space(space::M);
            ui.add(Label::new(title).selectable(false));
        },
    );
}

pub fn menu_separator(ui: &mut Ui) {
    ui.add(Separator::default().spacing(space::M));
}

pub fn bar_separator(ui: &mut Ui) {
    ui.add(Separator::default().vertical().spacing(space::M));
}

pub fn row_background(ui: &Ui, rect: Rect, clip: Rect, selected: bool) {
    let hovered = ui.rect_contains_pointer(rect);
    let fill = theme::palette(ui.visuals()).row(selected, hovered);
    egui::Painter::new(ui.ctx().clone(), ui.layer_id(), clip).rect_filled(
        rect,
        CornerRadius::same(radius::M),
        fill,
    );
}

pub fn list_row(ui: &mut Ui, selected: bool, add: impl FnOnce(&mut Ui)) -> Response {
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), size::CONTROL), Sense::click());
    row_background(ui, rect, ui.clip_rect(), selected);
    overlay(
        ui,
        rect.shrink2(vec2(space::M, 0.0)),
        Layout::left_to_right(Align::Center),
        add,
    );
    response
}

pub fn segmented_width(ui: &Ui, options: &[(&str, Option<String>)]) -> f32 {
    let font = TextStyle::Button.resolve(ui.style());
    let buttons: f32 = options
        .iter()
        .map(|(text, _)| {
            let text = ui
                .painter()
                .layout_no_wrap((*text).to_owned(), font.clone(), Color32::TRANSPARENT)
                .size()
                .x;
            (text.ceil() + 2.0 * space::M).max(size::CONTROL)
        })
        .sum();
    buttons + space::XS * options.len().saturating_sub(1) as f32
}

pub fn segmented(
    ui: &mut Ui,
    options: &[(&str, Option<String>)],
    selected: usize,
    stretch: bool,
) -> Option<usize> {
    let (fill, _) = theme::palette(ui.visuals()).item(State::Selected);
    let gaps = space::XS * options.len().saturating_sub(1) as f32;
    let width = if stretch {
        (ui.available_width() - gaps) / options.len() as f32
    } else {
        size::CONTROL
    };
    let mut chosen = None;
    let row = |ui: &mut Ui| {
        ui.spacing_mut().item_spacing.x = space::XS;
        ui.spacing_mut().button_padding = vec2(space::M, space::S);
        let mut order: Vec<usize> = (0..options.len()).collect();
        if ui.layout().prefer_right_to_left() {
            order.reverse();
        }
        for i in order {
            let (text, tooltip) = &options[i];
            let button = ghost(*text, true).min_size(vec2(width, size::CONTROL));
            let button = if i == selected {
                button.fill(fill)
            } else {
                button
            };
            let response = ui.add(button);
            let response = match tooltip {
                Some(tip) => response.on_hover_text(tip),
                None => response,
            };
            if response.clicked() {
                chosen = Some(i);
            }
        }
    };
    if ui.layout().is_horizontal() {
        ui.scope(row);
    } else {
        ui.horizontal(row);
    }
    chosen
}

pub fn file_card<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    let p = theme::palette(ui.visuals());
    surface(ui, p.panel, p.panel, radius::M, |ui| {
        ui.set_min_width(ui.available_width());
        ui.spacing_mut().item_spacing.y = 0.0;
        add(ui)
    })
}

pub fn card_header(ui: &mut Ui, add: impl FnOnce(&mut Ui)) -> Response {
    let (rect, response) = ui.allocate_exact_size(
        vec2(ui.available_width(), size::CONTROL + space::M),
        Sense::click(),
    );
    overlay(
        ui,
        rect.shrink2(vec2(space::M, 0.0)),
        Layout::left_to_right(Align::Center),
        add,
    );
    response
}

pub fn divider(ui: &mut Ui) {
    ui.add(Separator::default().spacing(0.0));
}

pub fn bar<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    Frame::new()
        .fill(theme::palette(ui.visuals()).hunk)
        .inner_margin(Margin::symmetric(space::M as i8, 0))
        .show(ui, |ui| bar_row(ui, size::CONTROL, add))
        .inner
}

pub fn number(ui: &mut Ui, value: impl ToString) {
    let color = theme::palette(ui.visuals()).text_faint;
    let text = RichText::new(value.to_string()).monospace().color(color);
    ui.add(Label::new(text).selectable(false));
}

pub fn divergence(ui: &mut Ui, upstream: &Upstream) {
    let color = theme::palette(ui.visuals()).text_faint;
    let (text, small) = match (upstream.ahead, upstream.behind) {
        (Some(ahead), Some(behind)) => (counts(ahead, behind), false),
        _ => (fl!("upstream-gone"), true),
    };
    if text.is_empty() {
        return;
    }
    let text = if small {
        RichText::new(text).small()
    } else {
        RichText::new(text).monospace()
    };
    ui.add(Label::new(text.color(color)).selectable(false))
        .on_hover_text(upstream.name.clone());
}

fn counts(ahead: u32, behind: u32) -> String {
    let mut parts = Vec::new();
    if ahead > 0 {
        parts.push(format!("↑{}", limit(ahead)));
    }
    if behind > 0 {
        parts.push(format!("↓{}", limit(behind)));
    }
    parts.join(" ")
}

fn limit(value: u32) -> String {
    if value >= DIVERGENCE_LIMIT {
        format!("{}+", DIVERGENCE_LIMIT - 1)
    } else {
        value.to_string()
    }
}

pub fn section(
    ui: &mut Ui,
    text: impl Into<String>,
    count: Option<usize>,
    add: impl FnOnce(&mut Ui),
) {
    ui.add_space(space::L);
    bar_row(ui, size::CONTROL, |ui| {
        let title = heading(ui, text);
        ui.add(Label::new(title).selectable(false));
        if let Some(count) = count {
            number(ui, count);
        }
        ui.with_layout(Layout::right_to_left(Align::Center), add);
    });
    ui.add_space(space::S);
}

pub fn empty(ui: &mut Ui, text: impl Into<String>) {
    let color = theme::palette(ui.visuals()).text_faint;
    ui.add_space(space::XL);
    ui.vertical_centered(|ui| {
        ui.add(Label::new(RichText::new(text).color(color)).selectable(false));
    });
    ui.add_space(space::XL);
}

pub fn logo(ui: &mut Ui) -> Response {
    let source = if ui.visuals().dark_mode {
        egui::include_image!("../../assets/brezza-mark-white.svg")
    } else {
        egui::include_image!("../../assets/brezza-mark-black.svg")
    };
    ui.add(egui::Image::new(source).fit_to_exact_size(Vec2::splat(size::LOGO)))
}

pub fn spinner(ui: &mut Ui) -> Response {
    ui.add(egui::Spinner::new().size(text::BODY))
}

pub fn loading(ui: &mut Ui) {
    ui.add_space(space::XL);
    ui.vertical_centered(spinner);
}

#[expect(clippy::disallowed_methods)]
pub fn input(ui: &mut Ui, value: &mut String, hint: impl Into<String>) -> Response {
    ui.add(
        egui::TextEdit::singleline(value)
            .hint_text(hint.into())
            .margin(Margin::symmetric(space::M as i8, (space::S + 1.0) as i8))
            .min_size(vec2(0.0, size::CONTROL))
            .desired_width(f32::INFINITY),
    )
}

#[expect(clippy::disallowed_methods)]
pub fn text_area(ui: &mut Ui, value: &mut String, hint: impl Into<String>) -> Response {
    ui.add(
        egui::TextEdit::multiline(value)
            .hint_text(hint.into())
            .margin(Margin::same(space::M as i8))
            .desired_rows(3)
            .desired_width(f32::INFINITY),
    )
}

pub fn checkbox(ui: &mut Ui, value: &mut bool, label: impl Into<String>) -> Response {
    let p = theme::palette(ui.visuals());
    let enabled = ui.is_enabled();
    let id = ui.next_auto_id();
    let row = ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = space::S + space::XS;
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(size::GLYPH), Sense::hover());
        let label = label.into();
        let label = if enabled {
            Label::new(label)
        } else {
            Label::new(RichText::new(label).color(p.text_faint))
        };
        ui.add(label.selectable(false));
        rect
    });
    let response = ui.interact(row.response.rect, id, Sense::click());
    if enabled && response.clicked() {
        *value = !*value;
    }
    let (fill, stroke) = match (*value, enabled) {
        (_, false) => (p.control, p.border),
        (true, true) => (p.accent, p.accent),
        (false, true) if response.hovered() => (p.input, p.text_faint),
        (false, true) => (p.input, p.border_strong),
    };
    ui.painter().rect(
        row.inner,
        CornerRadius::same(radius::S),
        fill,
        Stroke::new(1.0, stroke),
        egui::StrokeKind::Inside,
    );
    if *value {
        let color = if enabled { p.on_accent } else { p.text_faint };
        ui.painter().text(
            row.inner.center(),
            egui::Align2::CENTER_CENTER,
            icons::CHECK,
            egui::FontId::proportional(text::CAPTION),
            color,
        );
    }
    response
}

pub fn link(ui: &mut Ui, text: impl Into<egui::WidgetText>) -> Response {
    ui.add(egui::Link::new(text))
}

pub fn dialog_actions(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    ui.add_space(space::L);
    ui.spacing_mut().item_spacing.x = space::S;
    ui.allocate_ui_with_layout(
        vec2(ui.available_width(), size::CONTROL),
        Layout::right_to_left(Align::Center).with_main_wrap(true),
        add,
    );
}

pub enum NoticeAction {
    Dismissed,
    Action(usize),
}

pub fn notice(
    ui: &mut Ui,
    text: &str,
    actions: &[String],
    dismiss: impl Into<String>,
) -> Option<NoticeAction> {
    let p = theme::palette(ui.visuals());
    let mut result = None;
    bar_row(ui, size::HEADER, |ui| {
        let mark = RichText::new(icons::CIRCLE_ALERT).color(p.danger);
        ui.add(Label::new(mark).selectable(false));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if icon(ui, icons::X, dismiss).clicked() {
                result = Some(NoticeAction::Dismissed);
            }
            for (i, label) in actions.iter().enumerate().rev() {
                if ui.add(ghost(label.as_str(), false)).clicked() {
                    result = Some(NoticeAction::Action(i));
                }
            }
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                ui.add(Label::new(RichText::new(text).color(p.text)).truncate());
            });
        });
    });
    result
}

pub fn banner<R>(
    ui: &mut Ui,
    icon: &str,
    text: impl Into<String>,
    add: impl FnOnce(&mut Ui) -> R,
) -> R {
    let p = theme::palette(ui.visuals());
    let text = text.into();
    Frame::new()
        .fill(p.raised)
        .stroke(Stroke::new(1.0, p.border_strong))
        .corner_radius(CornerRadius::same(radius::M))
        .inner_margin(Margin::symmetric(space::M as i8, space::S as i8))
        .show(ui, |ui| {
            bar_row(ui, size::CONTROL, |ui| {
                let mark = RichText::new(icon).color(p.warn);
                ui.add(Label::new(mark).selectable(false));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let result = add(ui);
                    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                        ui.add(Label::new(RichText::new(text).color(p.text)).truncate());
                    });
                    result
                })
                .inner
            })
        })
        .inner
}

pub fn object_label(
    ui: &mut Ui,
    icon: &str,
    tint: Option<Color32>,
    name: &str,
    detail: Option<&str>,
) {
    let p = theme::palette(ui.visuals());
    ui.spacing_mut().item_spacing.x = space::S + space::XS;
    let mark = RichText::new(icon).color(tint.unwrap_or(p.text_faint));
    ui.add(Label::new(mark).selectable(false));
    label_with_detail(ui, RichText::new(name).color(p.text), detail);
}

pub fn sidebar<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    Frame::new()
        .inner_margin(Margin::symmetric(space::M as i8, 0))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            add(ui)
        })
        .inner
}

pub fn badge(ui: &mut Ui, text: impl Into<String>, tone: Tone, max_width: f32) -> Response {
    let p = theme::palette(ui.visuals());
    let (fill, color) = match tone {
        Tone::Head => (p.accent, p.on_accent),
        Tone::Branch => (p.accent_soft, p.accent_text),
        Tone::Remote => (p.control, p.text_muted),
        Tone::Tag => (p.tag_soft, p.tag_text),
    };
    Frame::new()
        .fill(fill)
        .corner_radius(CornerRadius::same(radius::S))
        .inner_margin(Margin::symmetric((space::S + space::XS) as i8, 0))
        .show(ui, |ui| {
            ui.set_min_height(size::BADGE);
            ui.set_max_width((max_width - 2.0 * (space::S + space::XS)).max(1.0));
            ui.add(
                Label::new(RichText::new(text).small().color(color))
                    .selectable(false)
                    .truncate()
                    .show_tooltip_when_elided(false),
            );
        })
        .response
}

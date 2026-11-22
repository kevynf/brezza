use std::collections::HashMap;
use std::sync::Arc;

use egui::collapsing_header::CollapsingState;
use egui::{Align, Id, Label, Layout, RichText, Sense, vec2};
use egui_extras::{Column, TableBuilder};

use super::chrome::Tone;
use super::files::{Toggle, kind_icon, kind_tint, stat_pair};
use super::icons;
use super::time_fmt;
use super::{chrome, graph_view, theme};
use crate::fl;
use crate::git::protocol::{Command, ResetMode};
use crate::model::{ChangeKind, FileDiff, Head, RefInfo, RefKind, WorkingTree};
use crate::tab::{Dialog, ListView, NameAction, RepoTab, Selection};

const MAX_BADGES: usize = 2;

pub fn show(ui: &mut egui::Ui, tab: &mut RepoTab) {
    let views = [ListView::Commits, ListView::Files];
    let labels = [fl!("commits"), fl!("files")];
    chrome::pane_header(ui, |ui| {
        let tabs: Vec<chrome::Tab> = views
            .iter()
            .zip(&labels)
            .map(|(view, label)| chrome::Tab {
                icon: None,
                label: label.clone(),
                hint: None,
                selected: tab.view == *view,
                closable: false,
                pinned: false,
            })
            .collect();
        if let Some(chrome::TabAction::Select(i)) = ui.scope(|ui| chrome::tabs(ui, &tabs)).inner {
            tab.view = views[i];
        }
        if tab.view == ListView::Commits {
            ui.add_space(theme::space::S);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if let Some((at, total)) = tab.match_position() {
                    let at = at.map_or_else(|| "-".to_owned(), |a| a.to_string());
                    chrome::number(ui, format!("{at}/{total}"));
                }
                let response = chrome::search(ui, &mut tab.filter, fl!("search-commits"));
                if response.has_focus() {
                    let (back, next) = ui.input_mut(|i| {
                        (
                            i.consume_key(egui::Modifiers::SHIFT, egui::Key::Enter),
                            i.consume_key(egui::Modifiers::NONE, egui::Key::Enter),
                        )
                    });
                    if back || next {
                        tab.next_match(if back { -1 } else { 1 });
                    }
                }
            });
        } else {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let tree = Toggle::Tree.get(ui.ctx());
                let options = [
                    (icons::LIST, Some(fl!("flat-list"))),
                    (icons::LIST_TREE, Some(fl!("tree-view"))),
                ];
                if let Some(i) = chrome::segmented(ui, &options, usize::from(tree), false) {
                    Toggle::Tree.set(ui.ctx(), i == 1);
                }
                let all = Toggle::All.get(ui.ctx());
                let (changes_label, all_label) = (fl!("show-changes"), fl!("show-all"));
                let options = [(changes_label.as_str(), None), (all_label.as_str(), None)];
                if let Some(i) = chrome::segmented(ui, &options, usize::from(all), false) {
                    Toggle::All.set(ui.ctx(), i == 1);
                }
            });
        }
    });
    match tab.view {
        ListView::Commits => chrome::list_area(ui, |ui| commit_table(ui, tab)),
        ListView::Files => chrome::list_area(ui, |ui| file_list(ui, tab)),
    }
}

fn commit_table(ui: &mut egui::Ui, tab: &mut RepoTab) {
    let hits = tab.visible_commits().map(<[usize]>::to_vec);
    let Some(history) = &tab.history else {
        chrome::loading(ui);
        return;
    };
    let count = hits.as_ref().map_or(history.commits.len(), Vec::len);
    let at = |i: usize| hits.as_ref().map_or(i, |h| h[i]);
    let filtering = hits.is_some();
    let graph_width = graph_view::width(history.lanes);
    let title_height = theme::size::BADGE.max(ui.text_style_height(&egui::TextStyle::Body));
    let meta_height = ui.text_style_height(&egui::TextStyle::Small);
    let row_height = title_height + meta_height + theme::space::L + theme::space::S;
    let lead = (row_height - title_height - theme::space::XS - meta_height) / 2.0;
    let spacing = ui.spacing().item_spacing;
    let scroll_to = tab.scroll_to.take();
    let viewport = ui.available_rect_before_wrap().intersect(ui.clip_rect());
    let row_x = ui.available_rect_before_wrap().x_range();
    let background = |ui: &mut egui::Ui, selected: bool| {
        let rect = egui::Rect::from_x_y_ranges(row_x, ui.max_rect().y_range());
        chrome::row_background(ui, rect, viewport, selected);
    };
    let offset = history.graph.len().saturating_sub(history.commits.len());
    let graph_cell = |ui: &mut egui::Ui, g: Option<usize>| {
        let Some(g) = g.filter(|&g| g < history.graph.len()) else {
            return;
        };
        let rect = ui.max_rect().expand2(vec2(0.0, spacing.y * 0.5));
        let painter = egui::Painter::new(ui.ctx().clone(), ui.layer_id(), rect.intersect(viewport));
        let above = g.checked_sub(1).map(|a| &history.graph[a]);
        graph_view::paint_row(
            &painter,
            rect,
            above,
            &history.graph[g],
            g,
            !filtering,
            ui.visuals(),
        );
    };

    let mut clicked = None;
    let mut menu = None;
    let mut table = TableBuilder::new(ui)
        .id_salt("commits")
        .sense(Sense::click())
        .auto_shrink(false)
        .column(Column::exact(graph_width.max(theme::space::M)))
        .column(Column::remainder().clip(true));
    if let Some(row) = scroll_to {
        table = table.scroll_to_row(row, Some(Align::Center));
    }
    table.body(|body| {
        body.rows(row_height, count + 1, |mut row| {
            let index = row.index();
            row.set_hovered(false);
            if index == 0 {
                let selected = tab.selection == Selection::Working;
                row.col(|ui| {
                    background(ui, selected);
                    graph_cell(ui, (offset == 1).then_some(0));
                });
                row.col(|ui| working_row(ui, lead, title_height, tab.working.as_deref()));
                if row.response().clicked() {
                    clicked = Some(Selection::Working);
                }
                return;
            }
            let commit = &history.commits[at(index - 1)];
            let selected = tab.selection == Selection::Commit(commit.id);
            row.col(|ui| {
                background(ui, selected);
                graph_cell(ui, Some(at(index - 1) + offset));
            });
            row.col(|ui| {
                let refs = tab
                    .refs_by_commit
                    .get(&commit.id)
                    .map_or(&[][..], Vec::as_slice);
                let is_head = history.head == Head::Detached(commit.id)
                    || refs.iter().any(|r| is_head_ref(r, &history.head));
                let head_ref = refs.iter().find(|r| is_head_ref(r, &history.head));
                let mut shown = Vec::with_capacity(MAX_BADGES);
                if let Some(head_ref) = head_ref {
                    shown.push(head_ref);
                }
                let mut hidden = Vec::new();
                for r in refs {
                    if head_ref.is_some_and(|head| std::ptr::eq(head, r)) {
                        continue;
                    }
                    if shown.len() < MAX_BADGES {
                        shown.push(r);
                    } else {
                        hidden.push(r);
                    }
                }
                let badge_max = (ui.available_width() * 0.28).clamp(80.0, 180.0);
                ui.vertical(|ui| {
                    ui.add_space(lead);
                    ui.spacing_mut().item_spacing.y = theme::space::XS;
                    split_line(
                        ui,
                        title_height,
                        |ui| summary(ui, &commit.summary, is_head),
                        |ui| {
                            for r in &shown {
                                badge(ui, r, is_head_ref(r, &history.head), badge_max);
                            }
                            if !hidden.is_empty() {
                                let names: Vec<&str> =
                                    hidden.iter().map(|r| r.name.as_str()).collect();
                                chrome::badge(
                                    ui,
                                    format!("+{}", hidden.len()),
                                    chrome::Tone::Remote,
                                    60.0,
                                )
                                .on_hover_text(names.join("\n"));
                            }
                        },
                    );
                    split_line(
                        ui,
                        meta_height,
                        |ui| meta(ui, &commit.author),
                        |ui| meta(ui, &time_fmt::short(commit.time)),
                    );
                });
            });
            let response = row.response();
            if response.clicked() {
                clicked = Some(Selection::Commit(commit.id));
            }
            chrome::context_menu(&response, |ui| {
                menu = commit_menu(ui, commit.id, &commit.summary, &history.head)
            });
        });
    });

    if let Some(selection) = clicked {
        tab.select(selection);
    }
    if let Some(action) = menu {
        match action {
            MenuAction::Run(command) => tab.run(command),
            MenuAction::Integrate(command) => tab.integrate(command),
            MenuAction::Dialog(dialog) => tab.dialog = Some(dialog),
        }
    }
}

fn split_line(
    ui: &mut egui::Ui,
    height: f32,
    left: impl FnOnce(&mut egui::Ui),
    right: impl FnOnce(&mut egui::Ui),
) {
    let size = vec2(ui.available_width() - theme::space::L, height);
    ui.allocate_ui_with_layout(size, Layout::right_to_left(Align::Center), |ui| {
        ui.set_min_size(size);
        right(ui);
        ui.with_layout(Layout::left_to_right(Align::Center), left);
    });
}

fn meta(ui: &mut egui::Ui, text: &str) {
    let color = theme::palette(ui.visuals()).text_muted;
    ui.add(
        Label::new(RichText::new(text).small().color(color))
            .truncate()
            .selectable(false),
    );
}

fn working_row(ui: &mut egui::Ui, lead: f32, title_height: f32, w: Option<&WorkingTree>) {
    let p = theme::palette(ui.visuals());
    let title = match w {
        Some(w) if !w.is_clean() => fl!("uncommitted-changes"),
        Some(_) => fl!("no-changes"),
        None => fl!("commit-changes"),
    };
    ui.vertical(|ui| {
        ui.add_space(lead);
        ui.spacing_mut().item_spacing.y = theme::space::XS;
        split_line(
            ui,
            title_height,
            |ui| {
                ui.add(Label::new(RichText::new(title).strong()).selectable(false));
            },
            |ui| {
                let states = [
                    (
                        icons::FILE_QUESTION_MARK,
                        p.text_muted,
                        w.map_or(0, |w| w.untracked.len()),
                        fl!("untracked"),
                    ),
                    (
                        icons::FILE_PEN,
                        p.warn,
                        w.map_or(0, |w| w.unstaged.len()),
                        fl!("unstaged"),
                    ),
                    (
                        icons::FILE_CHECK,
                        p.added_text,
                        w.map_or(0, |w| w.staged.len()),
                        fl!("staged"),
                    ),
                ];
                for (glyph, tint, count, tooltip) in states {
                    chrome::counter(ui, glyph, tint, count, tooltip);
                }
            },
        );
        meta(ui, &fl!("commit-changes"));
    });
}

fn summary(ui: &mut egui::Ui, text: &str, strong: bool) {
    let text = RichText::new(text);
    let text = if strong { text.strong() } else { text };
    ui.add(Label::new(text).truncate().selectable(false));
}

fn is_head_ref(r: &RefInfo, head: &Head) -> bool {
    r.kind == RefKind::Local && matches!(head, Head::Branch(name) if *name == r.name)
}

fn badge(ui: &mut egui::Ui, r: &RefInfo, head: bool, max_width: f32) {
    let (icon, tone) = match r.kind {
        RefKind::Local if head => (icons::GIT_BRANCH, Tone::Head),
        RefKind::Local => (icons::GIT_BRANCH, Tone::Branch),
        RefKind::Remote => (icons::CLOUD, Tone::Remote),
        RefKind::Tag => (icons::TAG, Tone::Tag),
    };
    chrome::badge(ui, format!("{icon} {}", r.name), tone, max_width).on_hover_text(r.name.as_str());
}

pub enum MenuAction {
    Run(Command),
    Integrate(Command),
    Dialog(Dialog),
}

fn commit_menu(
    ui: &mut egui::Ui,
    id: crate::model::Oid,
    summary: &str,
    head: &Head,
) -> Option<MenuAction> {
    let mut action = None;
    if chrome::menu_item(ui, fl!("checkout-commit")).clicked() {
        action = Some(MenuAction::Run(Command::CheckoutDetached(id)));
    }
    if chrome::menu_item(ui, fl!("create-branch")).clicked() {
        action = Some(MenuAction::Dialog(Dialog::Name {
            title: fl!("branch-name"),
            value: String::new(),
            action: NameAction::CreateBranch(id),
        }));
    }
    if chrome::menu_item(ui, fl!("create-tag")).clicked() {
        action = Some(MenuAction::Dialog(Dialog::Name {
            title: fl!("tag-name"),
            value: String::new(),
            action: NameAction::CreateTag(id),
        }));
    }
    if matches!(head, Head::Branch(_)) {
        chrome::menu_separator(ui);
        if chrome::menu_item(ui, fl!("merge-into-current")).clicked() {
            action = Some(MenuAction::Integrate(Command::Merge {
                target: id.to_string(),
                no_ff: false,
                autostash: false,
            }));
        }
        if chrome::menu_item(ui, fl!("merge-no-fast-forward")).clicked() {
            action = Some(MenuAction::Integrate(Command::Merge {
                target: id.to_string(),
                no_ff: true,
                autostash: false,
            }));
        }
        if chrome::menu_item(ui, fl!("rebase-onto")).clicked() {
            action = Some(MenuAction::Integrate(Command::Rebase {
                target: id.to_string(),
                autostash: false,
            }));
        }
    }
    chrome::menu_separator(ui);
    if chrome::menu_item(ui, fl!("cherry-pick")).clicked() {
        action = Some(MenuAction::Run(Command::CherryPick(id)));
    }
    if chrome::menu_item(ui, fl!("revert")).clicked() {
        action = Some(MenuAction::Run(Command::Revert(id)));
    }
    if chrome::menu_item(ui, fl!("reset-to-here")).clicked() {
        action = Some(MenuAction::Dialog(Dialog::Reset {
            id,
            summary: summary.to_owned(),
            mode: ResetMode::Mixed,
        }));
    }
    chrome::menu_separator(ui);
    if chrome::menu_item(ui, fl!("copy-hash")).clicked() {
        ui.ctx().copy_text(id.to_string());
    }
    action
}

fn file_list(ui: &mut egui::Ui, tab: &mut RepoTab) {
    let tree = Toggle::Tree.get(ui.ctx());
    let all = Toggle::All.get(ui.ctx());
    if all {
        tab.request_paths();
    }
    let mut focus = None;
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show_viewport(ui, |ui, viewport| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let focused = tab.active_file.as_deref();
            let groups: Vec<(&str, String, &[FileDiff])> = match tab.selection {
                Selection::Working => match &tab.working {
                    Some(w) => vec![
                        ("conflicts", fl!("conflicts"), &w.conflicts),
                        ("unstaged", fl!("unstaged"), &w.unstaged),
                        ("staged", fl!("staged"), &w.staged),
                        ("untracked", fl!("untracked"), &w.untracked),
                    ],
                    None => return chrome::loading(ui),
                },
                Selection::Commit(_) => match &tab.detail {
                    Some(d) => vec![("commit", fl!("changed-files"), &d.files)],
                    None => return chrome::loading(ui),
                },
            };
            let paths = if all {
                let Some(paths) = tab.paths() else {
                    return chrome::loading(ui);
                };
                Some(paths)
            } else {
                None
            };
            if paths.is_none() && groups.iter().all(|(_, _, files)| files.is_empty()) {
                chrome::empty(ui, fl!("no-changes"));
                return;
            }
            let cache_id = ui.make_persistent_id("files-cache");
            let generation_id = ui.make_persistent_id("files-generation");
            let generation = ui
                .ctx()
                .data_mut(|d| *d.get_temp_mut_or_default::<u64>(generation_id));
            let key = files_key(tab, &groups, all, tree, generation);
            let cached = ui.ctx().data_mut(|d| d.get_temp::<FilesCache>(cache_id));
            let cache = match cached {
                Some(cache) if cache.key == key => cache,
                _ => {
                    let rows = file_rows(ui, paths, &groups, tree);
                    let tops = chrome::row_tops(&rows);
                    let cache = FilesCache {
                        key,
                        rows: Arc::new(rows),
                        tops: Arc::new(tops),
                    };
                    ui.ctx()
                        .data_mut(|d| d.insert_temp(cache_id, cache.clone()));
                    cache
                }
            };
            chrome::virtual_rows(ui, &cache.tops, viewport, |ui, i| {
                file_row(ui, &cache.rows[i], tree, focused, &mut focus, generation_id);
            });
        });
    if let Some(path) = focus {
        tab.open_file(&path);
    }
}

struct FileLeaf {
    path: String,
    kind: Option<ChangeKind>,
    stats: Option<(u32, u32)>,
}

impl FileLeaf {
    fn new(path: &str, file: Option<&FileDiff>) -> Self {
        Self {
            path: path.to_owned(),
            kind: file.map(|file| file.kind.clone()),
            stats: file.and_then(FileDiff::stats),
        }
    }
}

#[derive(Clone)]
struct FilesCache {
    key: u64,
    rows: Arc<Vec<chrome::TreeRow<FileLeaf>>>,
    tops: Arc<Vec<f32>>,
}

fn file_rows(
    ui: &egui::Ui,
    all: Option<&[String]>,
    groups: &[(&str, String, &[FileDiff])],
    tree: bool,
) -> Vec<chrome::TreeRow<FileLeaf>> {
    let mut rows = Vec::new();
    if let Some(paths) = all {
        let mut changed: HashMap<&str, &FileDiff> = HashMap::new();
        for file in groups.iter().flat_map(|(_, _, files)| files.iter()) {
            changed.entry(file.path.as_str()).or_insert(file);
        }
        let mut entries: Vec<(&str, FileLeaf)> = paths
            .iter()
            .map(|path| {
                let file = changed.remove(path.as_str());
                (path.as_str(), FileLeaf::new(path, file))
            })
            .collect();
        entries.extend(
            changed
                .into_iter()
                .map(|(path, file)| (path, FileLeaf::new(path, Some(file)))),
        );
        entries.sort_unstable_by_key(|(path, _)| *path);
        let title = fl!("all-files");
        chrome::flatten(
            ui.ctx(),
            ui.make_persistent_id(("files", "all")),
            &title,
            entries,
            tree,
            &mut rows,
        );
        return rows;
    }
    for (key, title, files) in groups {
        if files.is_empty() {
            continue;
        }
        let entries = files
            .iter()
            .map(|file| (file.path.as_str(), FileLeaf::new(&file.path, Some(file))))
            .collect();
        let id = ui.make_persistent_id(("files", key));
        chrome::flatten(ui.ctx(), id, title, entries, tree, &mut rows);
    }
    rows
}

fn files_key(
    tab: &RepoTab,
    groups: &[(&str, String, &[FileDiff])],
    all: bool,
    tree: bool,
    generation: u64,
) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325;
    hash = mix(hash, generation);
    hash = mix(hash, u64::from(all));
    hash = mix(hash, u64::from(tree));
    for byte in crate::i18n::current().bytes() {
        hash = mix(hash, u64::from(byte));
    }
    match tab.selection {
        Selection::Working => hash = mix(hash, 0),
        Selection::Commit(id) => {
            hash = mix(hash, 1);
            for &byte in id.as_bytes() {
                hash = mix(hash, u64::from(byte));
            }
        }
    }
    if all && let Some(paths) = tab.paths() {
        hash = mix(hash, paths.len() as u64);
        hash = mix(hash, paths.as_ptr() as usize as u64);
        for path in paths {
            hash = mix(hash, path.len() as u64);
        }
    }
    for (_, _, files) in groups {
        hash = mix(hash, files.len() as u64);
        hash = mix(hash, files.as_ptr() as usize as u64);
        for file in files.iter() {
            hash = mix(hash, file.path.len() as u64);
            hash = mix(hash, kind_code(&file.kind));
            hash = mix(hash, file.version);
            hash = mix(
                hash,
                file.stats.map_or(u64::MAX, |(added, removed)| {
                    u64::from(added) << 32 | u64::from(removed)
                }),
            );
        }
    }
    hash
}

fn kind_code(kind: &ChangeKind) -> u64 {
    match kind {
        ChangeKind::Added => 1,
        ChangeKind::Deleted => 2,
        ChangeKind::Modified => 3,
        ChangeKind::Renamed(_) => 4,
        ChangeKind::Copied(_) => 5,
        ChangeKind::Conflicted(_) => 6,
        ChangeKind::Untracked => 13,
        ChangeKind::Unchanged => 14,
    }
}

fn mix(hash: u64, value: u64) -> u64 {
    (hash ^ value).wrapping_mul(0x0100_0000_01b3)
}

fn file_row(
    ui: &mut egui::Ui,
    row: &chrome::TreeRow<FileLeaf>,
    tree: bool,
    focused: Option<&str>,
    focus: &mut Option<String>,
    generation: Id,
) {
    match row {
        chrome::TreeRow::Heading {
            id,
            title,
            count,
            open,
        } => {
            if chrome::group_heading(ui, title, *count, *open, |_| {}).clicked() {
                toggle(ui, *id, generation);
            }
        }
        chrome::TreeRow::Folder {
            id,
            depth,
            name,
            open,
        } => {
            let row = chrome::Row {
                depth: *depth,
                icon: icons::FOLDER_OPEN,
                label: name,
                open: Some(*open),
                ..chrome::Row::default()
            };
            if chrome::nav_row(ui, row, |_| {}).clicked() {
                toggle(ui, *id, generation);
            }
        }
        chrome::TreeRow::Leaf { depth, name, item } => {
            let row = chrome::Row {
                depth: *depth,
                icon: item.kind.as_ref().map_or(icons::FILE, kind_icon),
                tint: item.kind.as_ref().map(|kind| kind_tint(ui, kind)),
                label: name,
                detail: if tree {
                    None
                } else {
                    item.path.rsplit_once('/').map(|(dir, _)| dir)
                },
                selected: focused == Some(item.path.as_str()),
                ..chrome::Row::default()
            };
            let stats = item.stats;
            let trailing = |ui: &mut egui::Ui| {
                if let Some((added, removed)) = stats {
                    stat_pair(ui, added, removed);
                }
            };
            if chrome::nav_row(ui, row, trailing).clicked() {
                *focus = Some(item.path.clone());
            }
        }
    }
}

fn toggle(ui: &egui::Ui, id: Id, generation: Id) {
    let mut state = CollapsingState::load_with_default_open(ui.ctx(), id, true);
    state.toggle(ui);
    state.store(ui.ctx());
    ui.ctx()
        .data_mut(|d| *d.get_temp_mut_or_default::<u64>(generation) += 1);
}

use super::diff_view::{SplitEdit, file_id, forget, lines_view, split_view, unified_edit};
use super::files::{Toggle, conflict_label, file_label, stat_pair, stats};
use super::icons;
use super::time_fmt;
use super::{chrome, theme};
use crate::diff::hunk_patch;
use crate::fl;
use crate::git::protocol::{Command, FileSource, GitError};
use crate::model::{
    ChangeKind, CommitDetail, Conflict, DiffBody, FileDiff, Hunk, InProgress, LAZY_LIMIT, LineKind,
    Operation, WorkingTree, default_open,
};
use crate::tab::{Dialog, EditState, OpenFile, RepoTab, Selection};
use egui::collapsing_header::CollapsingState;
use egui::{Align, Grid, Id, Key, KeyboardShortcut, Label, Layout, Modifiers, RichText};

const NEXT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::NONE, Key::F7);
const PREV: KeyboardShortcut = KeyboardShortcut::new(Modifiers::SHIFT, Key::F7);
const SAVE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::S);

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Area {
    Conflicted,
    Unstaged,
    Staged,
    Untracked,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Resolution {
    Ours,
    Theirs,
    Mark,
}

enum Event {
    Open,
    Stage,
    Unstage,
    Discard,
    Delete,
    Hunk(usize),
    Resolve(Resolution),
    MergeTool,
}

pub fn show(ui: &mut egui::Ui, tab: &mut RepoTab) {
    let folds = folds(tab);
    file_tabs(ui, tab, &folds);
    if let Some(path) = tab.active_file.clone() {
        full_file(ui, tab, &path);
        return;
    }
    egui::ScrollArea::vertical()
        .id_salt(tab.selection)
        .auto_shrink(false)
        .show(ui, |ui| {
            chrome::body(ui, |ui| match tab.selection {
                Selection::Working => working(ui, tab),
                Selection::Commit(_) => match tab.detail.take() {
                    Some(detail) => {
                        commit(ui, tab, &detail);
                        tab.detail = Some(detail);
                    }
                    None => chrome::loading(ui),
                },
            })
        });
}

fn section_id(path: &str, area: Option<Area>) -> Id {
    Id::NULL.with(("diff-section", path, area))
}

fn areas(w: &WorkingTree) -> Vec<(Area, String, &Vec<FileDiff>)> {
    [
        (Area::Conflicted, fl!("conflicts"), &w.conflicts),
        (Area::Unstaged, fl!("unstaged"), &w.unstaged),
        (Area::Staged, fl!("staged"), &w.staged),
        (Area::Untracked, fl!("untracked"), &w.untracked),
    ]
    .into_iter()
    .filter(|(_, _, files)| !files.is_empty())
    .collect()
}

fn folds(tab: &RepoTab) -> Vec<(Id, bool)> {
    match tab.selection {
        Selection::Working => tab.working.as_ref().map_or(Vec::new(), |w| {
            let areas = areas(w);
            let total = areas.iter().map(|(_, _, f)| f.len()).sum();
            areas
                .iter()
                .flat_map(|(area, _, files)| {
                    files.iter().map(move |f| {
                        (
                            section_id(&f.path, Some(*area)),
                            default_open(total, f.stats()),
                        )
                    })
                })
                .collect()
        }),
        Selection::Commit(_) => tab.detail.as_ref().map_or(Vec::new(), |d| {
            d.files
                .iter()
                .map(|f| {
                    (
                        section_id(&f.path, None),
                        default_open(d.files.len(), f.stats()),
                    )
                })
                .collect()
        }),
    }
}

fn set_folds(ctx: &egui::Context, folds: &[(Id, bool)], open: bool) {
    for (id, default_open) in folds {
        let mut state = CollapsingState::load_with_default_open(ctx, *id, *default_open);
        state.set_open(open);
        state.store(ctx);
    }
}

fn file_tabs(ui: &mut egui::Ui, tab: &mut RepoTab, folds: &[(Id, bool)]) {
    let summary = chrome::Tab {
        icon: None,
        label: fl!("summary"),
        hint: None,
        selected: tab.active_file.is_none(),
        closable: false,
        pinned: true,
    };
    let files = tab.open_files.iter().map(|file| {
        let mut label = file
            .path
            .rsplit('/')
            .next()
            .unwrap_or(&file.path)
            .to_owned();
        if file.edit.as_ref().is_some_and(|edit| edit.dirty) {
            label.push_str(" •");
        }
        chrome::Tab {
            icon: Some(icons::FILE),
            label,
            hint: Some(file.path.clone()),
            selected: tab.active_file.as_ref() == Some(&file.path),
            closable: true,
            pinned: false,
        }
    });
    let tabs: Vec<chrome::Tab> = std::iter::once(summary).chain(files).collect();
    let summary_open = tab.active_file.is_none();
    let folds_open = summary_open && !folds.is_empty();
    let trailing = if folds_open {
        2.0 * theme::size::CONTROL + 2.0 * theme::space::M
    } else {
        0.0
    };
    let action = chrome::pane_header(ui, |ui| {
        let action = ui
            .scope(|ui| {
                ui.set_max_width((ui.available_width() - trailing).max(0.0));
                chrome::tabs(ui, &tabs)
            })
            .inner;
        if folds_open {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if chrome::icon(ui, icons::FOLD_VERTICAL, fl!("collapse-all")).clicked() {
                    set_folds(ui.ctx(), folds, false);
                }
                if chrome::icon(ui, icons::UNFOLD_VERTICAL, fl!("expand-all")).clicked() {
                    set_folds(ui.ctx(), folds, true);
                }
            });
        }
        action
    });
    match action {
        Some(chrome::TabAction::Select(0)) => tab.active_file = None,
        Some(chrome::TabAction::Select(i)) => {
            let path = tab.open_files[i - 1].path.clone();
            tab.open_file(&path);
        }
        Some(chrome::TabAction::Close(i)) => {
            let path = tab.open_files[i - 1].path.clone();
            tab.close_file(&path);
            forget(ui.ctx(), &path);
        }
        None => {}
    }
}

struct FileMeta {
    area: Option<Area>,
    dirty: bool,
    editable: bool,
    changes: Vec<usize>,
    text: bool,
    wrap: bool,
    split: bool,
    merge_tool: bool,
    unchanged: bool,
    disk_changed: bool,
    preview_error: Option<GitError>,
    working_copy: bool,
    dismissed: bool,
    dismiss_id: Id,
}

struct FileView<'a> {
    diff: &'a FileDiff,
    edit: &'a mut Option<EditState>,
    change: &'a mut usize,
    jump: &'a mut Option<usize>,
}

#[derive(Default)]
struct FileActions {
    event: Option<Event>,
    save: bool,
    reload: bool,
    reload_confirm: bool,
    overwrite: bool,
    dismiss: bool,
}

fn file_header(
    ui: &mut egui::Ui,
    view: &mut FileView<'_>,
    meta: &FileMeta,
    actions: &mut FileActions,
    path: &str,
) {
    chrome::pane_header(ui, |ui| {
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if let Some(area) = meta.area {
                if area == Area::Conflicted {
                    actions.event = conflict_menu(ui, meta.merge_tool);
                } else {
                    actions.event = area_buttons(ui, area, false);
                }
                chrome::bar_separator(ui);
            }
            if meta.text {
                let options = [
                    (icons::ROWS_3, Some(fl!("diff-unified"))),
                    (icons::COLUMNS_2, Some(fl!("diff-split"))),
                ];
                if let Some(i) = chrome::segmented(ui, &options, usize::from(meta.split), false) {
                    let next = i == 1;
                    if next != meta.split {
                        Toggle::Split.set(ui.ctx(), next);
                        forget(ui.ctx(), path);
                    }
                }
            }
            if meta.text {
                let options = [
                    (icons::MOVE_HORIZONTAL, Some(fl!("scroll-lines"))),
                    (icons::WRAP_TEXT, Some(fl!("wrap-lines"))),
                ];
                if let Some(i) = chrome::segmented(ui, &options, usize::from(meta.wrap), false) {
                    Toggle::Wrap.set(ui.ctx(), i == 1);
                }
            }
            if meta.dirty && chrome::icon(ui, icons::FILE_CHECK, fl!("save")).clicked() {
                actions.save = true;
            }
            navigation(ui, view.change, view.jump, &meta.changes);
            if !meta.unchanged {
                chrome::bar_separator(ui);
                stats(ui, view.diff);
            }
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                ui.add_space(theme::space::M);
                file_label(ui, view.diff);
                if let ChangeKind::Conflicted(conflict) = &view.diff.kind {
                    conflict_note(ui, *conflict, view.diff.conflict_markers);
                }
                if meta.working_copy {
                    let muted = theme::palette(ui.visuals()).text_muted;
                    let note = fl!("editing-working-tree");
                    ui.add(
                        Label::new(RichText::new(note.as_str()).small().color(muted))
                            .truncate()
                            .selectable(false),
                    )
                    .on_hover_text(note);
                }
            });
        });
    });
}

fn file_banners(ui: &mut egui::Ui, meta: &FileMeta, actions: &mut FileActions) {
    let disk = meta.disk_changed && !meta.dismissed;
    if !disk && meta.preview_error.is_none() {
        return;
    }
    chrome::body(ui, |ui| {
        if disk {
            chrome::banner(ui, icons::CIRCLE_ALERT, fl!("file-changed-on-disk"), |ui| {
                if chrome::icon(ui, icons::X, fl!("dismiss")).clicked() {
                    actions.dismiss = true;
                }
                if meta.dirty {
                    if chrome::text_button(ui, chrome::Kind::Secondary, fl!("reload")).clicked() {
                        actions.reload_confirm = true;
                    }
                    if chrome::text_button(ui, chrome::Kind::Primary, fl!("overwrite")).clicked() {
                        actions.overwrite = true;
                    }
                } else if chrome::text_button(ui, chrome::Kind::Primary, fl!("reload")).clicked() {
                    actions.reload = true;
                }
            });
        }
        if let Some(error) = &meta.preview_error {
            if disk {
                ui.add_space(theme::space::S);
            }
            preview_warning(ui, &error.message);
        }
    });
}

fn file_body(
    ui: &mut egui::Ui,
    view: &mut FileView<'_>,
    meta: &FileMeta,
    path: &str,
    selection: Selection,
) -> bool {
    let mut changed = false;
    let target = view.jump.take();
    match &view.diff.body {
        DiffBody::Binary => chrome::empty(ui, fl!("binary-file")),
        DiffBody::TooLarge => chrome::empty(ui, fl!("file-too-large")),
        DiffBody::Pending => chrome::loading(ui),
        DiffBody::Text(hunks) => {
            let shown = view
                .edit
                .as_ref()
                .and_then(|edit| edit.preview.clone())
                .unwrap_or_else(|| hunks.clone());
            if meta.split {
                let split_edit = if meta.editable {
                    view.edit.as_mut().map(|edit| SplitEdit {
                        text: &mut edit.text,
                        sizes: &mut edit.rows,
                    })
                } else {
                    None
                };
                changed |= split_view(ui, path, &shown, target, meta.wrap, split_edit);
            } else {
                let scroll = if meta.wrap {
                    egui::ScrollArea::vertical()
                } else {
                    egui::ScrollArea::both()
                };
                scroll
                    .id_salt((selection, path))
                    .auto_shrink(false)
                    .show(ui, |ui| {
                        if meta.editable {
                            if let Some(edit) = view.edit.as_mut() {
                                ui.add_space(theme::space::S);
                                changed |= unified_edit(
                                    ui,
                                    &mut edit.text,
                                    &shown,
                                    target,
                                    meta.wrap,
                                    ui.available_width(),
                                    file_id(path).with("edit"),
                                    &mut edit.rows,
                                );
                            }
                        } else if let Some(hunk) = shown.first() {
                            ui.add_space(theme::space::S);
                            lines_view(ui, &hunk.lines, target, meta.wrap, file_id(path));
                        }
                    });
            }
        }
    }
    changed
}

fn full_file(ui: &mut egui::Ui, tab: &mut RepoTab, path: &str) {
    let selection = tab.selection;
    let merge_tool = tab.working.as_ref().is_some_and(|w| w.merge_tool);
    let wrap = Toggle::Wrap.get(ui.ctx());
    let split = Toggle::Split.get(ui.ctx());
    let Some(file) = tab.open_files.iter_mut().find(|f| f.path == path) else {
        chrome::loading(ui);
        return;
    };
    let OpenFile {
        diff: Some(diff),
        source,
        edit_source,
        edit,
        change,
        jump,
        ..
    } = file
    else {
        chrome::loading(ui);
        return;
    };
    let source = *source;
    let edit_source = *edit_source;
    let unchanged = diff.kind == ChangeKind::Unchanged;
    let area = match source {
        _ if unchanged => None,
        FileSource::Commit(_) => None,
        FileSource::Conflicted => Some(Area::Conflicted),
        FileSource::Staged => Some(Area::Staged),
        FileSource::Unstaged => Some(Area::Unstaged),
        FileSource::Untracked => Some(Area::Untracked),
    };
    let dirty = edit.as_ref().is_some_and(|edit| edit.dirty);
    let editable = matches!(
        edit_source,
        FileSource::Conflicted | FileSource::Unstaged | FileSource::Untracked
    ) && edit.is_some()
        && (matches!(diff.body, DiffBody::Text(_))
            || edit.as_ref().is_some_and(|edit| edit.preview.is_some()))
        && (diff.kind != ChangeKind::Deleted || dirty);
    let changes = edit
        .as_ref()
        .and_then(|edit| edit.preview.as_deref())
        .map_or_else(|| diff.change_starts(), |hunks| change_starts(hunks));
    let text = matches!(diff.body, DiffBody::Text(_) | DiffBody::Pending)
        || edit.as_ref().is_some_and(|edit| edit.preview.is_some());
    let disk_changed = edit.as_ref().is_some_and(|edit| edit.disk_changed);
    let preview_error = edit.as_ref().and_then(|edit| edit.preview_error.clone());
    let working_copy = edit.is_some() && edit_source != source;
    let dismiss_id = Id::NULL.with(("disk-changed-dismissed", path, diff.version));
    let dismissed = ui
        .ctx()
        .data(|data| data.get_temp::<bool>(dismiss_id).unwrap_or_default());
    let save = dirty && ui.input_mut(|input| input.consume_shortcut(&SAVE));
    let meta = FileMeta {
        area,
        dirty,
        editable,
        changes,
        text,
        wrap,
        split,
        merge_tool,
        unchanged,
        disk_changed,
        preview_error,
        working_copy,
        dismissed,
        dismiss_id,
    };
    let mut view = FileView {
        diff,
        edit,
        change,
        jump,
    };
    let mut actions = FileActions {
        save,
        ..Default::default()
    };
    file_header(ui, &mut view, &meta, &mut actions, path);
    file_banners(ui, &meta, &mut actions);
    let changed = file_body(ui, &mut view, &meta, path, selection);
    if changed && let Some(edit) = view.edit.as_mut() {
        edit.touch(ui.input(|input| input.time));
        ui.ctx().request_repaint();
    }
    let (mut run, mut dialog) = (None, None);
    if let (Some(event), Some(area)) = (actions.event, meta.area) {
        match event {
            Event::Resolve(resolution) => {
                let files = std::slice::from_ref(view.diff);
                if let Some(confirm) = resolve_dialog(files, resolution) {
                    dialog = Some(confirm);
                } else {
                    run = Some(resolve(files, resolution));
                }
            }
            Event::MergeTool => run = Some(Command::MergeTool(path.to_owned())),
            event => apply(event, area, &[path.to_owned()], None, &mut run, &mut dialog),
        }
    }
    if actions.dismiss {
        ui.ctx()
            .data_mut(|data| data.insert_temp(meta.dismiss_id, true));
    }
    let mut save_error = None;
    if actions.reload {
        match tab.reload_file(path) {
            Ok(()) => ui
                .ctx()
                .data_mut(|data| data.remove::<bool>(meta.dismiss_id)),
            Err(error) => save_error = Some(error),
        }
    }
    if actions.overwrite
        && let Err(error) = tab.save_file(path, true)
    {
        save_error = Some(error);
    }
    if actions.save
        && dirty
        && let Err(error) = tab.save_file(path, false)
    {
        save_error = Some(error);
    }
    if let Some(error) = save_error {
        ui.ctx()
            .data_mut(|data| data.remove::<bool>(meta.dismiss_id));
        tab.error = Some(error);
    }
    if let Some(command) = run {
        tab.run(command);
    }
    if actions.reload_confirm {
        tab.dialog = Some(Dialog::Reload {
            path: path.to_owned(),
        });
    } else if dialog.is_some() {
        tab.dialog = dialog;
    }
}

fn preview_warning(ui: &mut egui::Ui, message: &str) {
    let color = theme::palette(ui.visuals()).danger;
    ui.horizontal(|ui| {
        let mark = RichText::new(icons::CIRCLE_ALERT).color(color);
        ui.add(Label::new(mark).selectable(false));
        let text = fl!("preview-error", message = message);
        ui.add(Label::new(RichText::new(text).small().color(color)).wrap());
    });
}

fn change_starts(hunks: &[Hunk]) -> Vec<usize> {
    let Some(lines) = hunks.first().map(|hunk| &hunk.lines) else {
        return Vec::new();
    };
    (0..lines.len())
        .filter(|&i| {
            lines[i].kind != LineKind::Context && (i == 0 || lines[i - 1].kind == LineKind::Context)
        })
        .collect()
}

fn navigation(ui: &mut egui::Ui, change: &mut usize, jump: &mut Option<usize>, changes: &[usize]) {
    let count = changes.len();
    if count == 0 {
        return;
    }
    *change = (*change).min(count - 1);
    let tip = |label: String, shortcut: &KeyboardShortcut| {
        format!("{label} ({})", ui.ctx().format_shortcut(shortcut))
    };
    let (prev_tip, next_tip) = (
        tip(fl!("previous-change"), &PREV),
        tip(fl!("next-change"), &NEXT),
    );
    let next = chrome::icon(ui, icons::CHEVRON_DOWN, next_tip).clicked();
    let prev = chrome::icon(ui, icons::CHEVRON_UP, prev_tip).clicked();
    let (key_prev, key_next) =
        ui.input_mut(|i| (i.consume_shortcut(&PREV), i.consume_shortcut(&NEXT)));
    let delta = if prev || key_prev {
        -1
    } else if next || key_next {
        1
    } else {
        0
    };
    if delta != 0 {
        *change = (*change as isize + delta).rem_euclid(count as isize) as usize;
        *jump = Some(changes[*change]);
    }
    chrome::number(ui, format!("{}/{count}", *change + 1));
}

fn commit(ui: &mut egui::Ui, tab: &mut RepoTab, detail: &CommitDetail) {
    let muted = theme::palette(ui.visuals()).text_muted;
    let field = |ui: &mut egui::Ui, name: String| {
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(RichText::new(name).color(muted));
        });
    };
    Grid::new("commit-fields").num_columns(2).show(ui, |ui| {
        field(ui, fl!("commit-hash"));
        ui.monospace(detail.id.to_string());
        ui.end_row();
        field(ui, fl!("author"));
        ui.label(format!("{} <{}>", detail.author.name, detail.author.email));
        ui.end_row();
        field(ui, fl!("date"));
        ui.label(time_fmt::full(detail.author.time));
        ui.end_row();
        if detail.committer.name != detail.author.name {
            field(ui, fl!("committer"));
            ui.label(format!(
                "{} <{}>",
                detail.committer.name, detail.committer.email
            ));
            ui.end_row();
        }
        if !detail.parents.is_empty() {
            field(ui, fl!("parents"));
            ui.horizontal(|ui| {
                for p in &detail.parents {
                    let text = RichText::new(p.to_hex_with_len(10).to_string()).monospace();
                    if chrome::link(ui, text).clicked() {
                        tab.reveal(*p);
                    }
                }
            });
            ui.end_row();
        }
    });
    ui.add_space(theme::space::M);
    ui.add(Label::new(RichText::new(detail.message.trim_end()).monospace()).wrap());

    let (added, removed) = detail
        .files
        .iter()
        .filter_map(FileDiff::stats)
        .fold((0, 0), |a, b| (a.0 + b.0, a.1 + b.1));
    let title = fl!("changed-files");
    let id = Id::NULL.with("summary-commit");
    let total = detail.files.len();
    if !chrome::group(ui, id, &title, total, true, |ui| {
        stat_pair(ui, added, removed)
    }) {
        return;
    }
    for file in &detail.files {
        let id = section_id(&file.path, None);
        let source = FileSource::Commit(detail.id);
        if let Some(Event::Open) = file_section(
            ui,
            tab,
            file,
            None,
            source,
            id,
            default_open(total, file.stats()),
        ) {
            tab.open_file(&file.path);
        }
    }
}

fn working(ui: &mut egui::Ui, tab: &mut RepoTab) {
    let Some(w) = tab.working.clone() else {
        chrome::loading(ui);
        return;
    };
    let areas = areas(&w);
    let total: usize = areas.iter().map(|(_, _, f)| f.len()).sum();
    let mut run = None;
    let mut dialog = None;
    let mut open = None;

    if let Some(op) = w.in_progress.clone() {
        banner(ui, tab, &w, &op, &mut run, &mut dialog);
        ui.add_space(theme::space::M);
    }
    commit_box(
        ui,
        tab,
        !w.staged.is_empty(),
        w.in_progress.is_none(),
        &mut run,
    );
    if areas.is_empty() {
        chrome::empty(ui, fl!("no-changes"));
    }
    for (area, title, files) in areas {
        let all: Vec<String> = files.iter().map(|f| f.path.clone()).collect();
        let mut event = None;
        let id = Id::NULL.with(("summary-area", area));
        let initial_open = files.len() <= LAZY_LIMIT;
        let expanded = chrome::group(ui, id, &title, files.len(), initial_open, |ui| {
            event = area_buttons(ui, area, true);
        });
        if let Some(e) = event {
            match e {
                Event::Resolve(resolution) => {
                    if let Some(confirm) = resolve_dialog(files, resolution) {
                        dialog = Some(confirm);
                    } else {
                        run = Some(resolve(files, resolution));
                    }
                }
                e => apply(e, area, &all, None, &mut run, &mut dialog),
            }
        }
        if !expanded {
            continue;
        }
        let source = source(area);
        for file in files {
            let id = section_id(&file.path, Some(area));
            match file_section(
                ui,
                tab,
                file,
                Some(area),
                source,
                id,
                default_open(total, file.stats()),
            ) {
                Some(Event::Open) => open = Some(file.path.clone()),
                Some(Event::Resolve(resolution)) => {
                    let files = std::slice::from_ref(file);
                    if let Some(confirm) = resolve_dialog(files, resolution) {
                        dialog = Some(confirm);
                    } else {
                        run = Some(resolve(files, resolution));
                    }
                }
                Some(Event::MergeTool) => run = Some(Command::MergeTool(file.path.clone())),
                Some(e @ Event::Hunk(_)) => {
                    let hunks = tab.hunks(source, file);
                    apply(
                        e,
                        area,
                        std::slice::from_ref(&file.path),
                        hunks.as_ref().map(|hunks| hunks.as_slice()),
                        &mut run,
                        &mut dialog,
                    );
                }
                Some(e) => apply(
                    e,
                    area,
                    std::slice::from_ref(&file.path),
                    None,
                    &mut run,
                    &mut dialog,
                ),
                None => {}
            }
        }
    }
    if let Some(path) = open {
        tab.open_file(&path);
    }
    if let Some(command) = run {
        tab.run(command);
    }
    if dialog.is_some() {
        tab.dialog = dialog;
    }
}

fn commit_box(
    ui: &mut egui::Ui,
    tab: &mut RepoTab,
    staged: bool,
    allow_amend: bool,
    run: &mut Option<Command>,
) {
    chrome::text_area(ui, &mut tab.message, fl!("commit-message"));
    ui.add_space(theme::space::S);
    if !allow_amend {
        tab.amend = false;
    }
    ui.horizontal(|ui| {
        ui.add_enabled_ui(allow_amend, |ui| {
            chrome::checkbox(ui, &mut tab.amend, fl!("amend"));
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let ready = !tab.message.trim().is_empty() && (tab.amend || staged);
            let shortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Enter);
            let pressed = ready && ui.input_mut(|i| i.consume_shortcut(&shortcut));
            let clicked = ui
                .add_enabled_ui(ready, |ui| {
                    chrome::button(ui, chrome::Kind::Primary, icons::CHECK, fl!("commit"))
                })
                .inner
                .on_hover_text(ui.ctx().format_shortcut(&shortcut))
                .clicked();
            if clicked || pressed {
                let ready = if tab.has_dirty() {
                    match tab.save_dirty() {
                        Ok(()) => true,
                        Err(error) => {
                            tab.error = Some(error);
                            false
                        }
                    }
                } else {
                    true
                };
                if ready {
                    *run = Some(Command::Commit {
                        message: tab.message.trim().to_owned(),
                        amend: tab.amend,
                    });
                    tab.message.clear();
                    tab.amend = false;
                }
            }
        });
    });
}

fn banner(
    ui: &mut egui::Ui,
    tab: &RepoTab,
    w: &WorkingTree,
    op: &InProgress,
    run: &mut Option<Command>,
    dialog: &mut Option<Dialog>,
) {
    let text = operation_text(op, w.conflicts.len());
    chrome::banner(ui, operation_icon(op.operation), text, |ui| {
        if op.operation == Operation::Bisect {
            if chrome::text_button(ui, chrome::Kind::Ghost, fl!("end-bisect")).clicked() {
                *run = Some(Command::Abort(Operation::Bisect));
            }
            return;
        }
        let message = continue_message(tab, op);
        let ready = w.conflicts.is_empty() && (message.is_some() || !commit_path(op));
        let clicked = ui
            .add_enabled_ui(ready, |ui| {
                chrome::text_button(ui, chrome::Kind::Primary, fl!("continue"))
            })
            .inner
            .clicked();
        if clicked {
            *run = Some(Command::Continue {
                operation: op.operation,
                sequence: op.sequence,
                message: message.unwrap_or_default(),
            });
        }
        if can_skip(op) && chrome::text_button(ui, chrome::Kind::Secondary, fl!("skip")).clicked() {
            *run = Some(Command::Skip(op.operation));
        }
        if chrome::text_button(ui, chrome::Kind::Ghost, fl!("abort")).clicked() {
            *dialog = Some(Dialog::Confirm {
                text: fl!("confirm-abort"),
                icon: icons::X,
                button: fl!("abort"),
                danger: true,
                command: Command::Abort(op.operation),
            });
        }
    });
}

fn operation_text(op: &InProgress, conflicts: usize) -> String {
    let label = match op.operation {
        Operation::Merge => op.target.as_deref().map_or_else(
            || fl!("operation-merging"),
            |name| fl!("operation-merge", name = name),
        ),
        Operation::Rebase => op.step.map_or_else(
            || fl!("operation-rebasing"),
            |(step, total)| fl!("operation-rebase-step", step = step, total = total),
        ),
        Operation::CherryPick => op.commit.map_or_else(
            || fl!("operation-cherry-picking"),
            |id| fl!("operation-cherry-pick", commit = short(id)),
        ),
        Operation::Revert => op.commit.map_or_else(
            || fl!("operation-reverting"),
            |id| fl!("operation-revert", commit = short(id)),
        ),
        Operation::Bisect => fl!("operation-bisect"),
        Operation::ApplyMailbox => op.step.map_or_else(
            || fl!("operation-apply"),
            |(step, total)| fl!("operation-apply-step", step = step, total = total),
        ),
    };
    if conflicts == 0 {
        label
    } else {
        fl!("operation-conflicts", operation = label, count = conflicts)
    }
}

fn operation_icon(operation: Operation) -> &'static str {
    match operation {
        Operation::Merge => icons::GIT_MERGE,
        Operation::Rebase => icons::GIT_BRANCH,
        Operation::CherryPick => icons::GIT_COMMIT_HORIZONTAL,
        Operation::Revert => icons::UNDO_2,
        Operation::Bisect => icons::SEARCH,
        Operation::ApplyMailbox => icons::MAIL,
    }
}

fn commit_path(op: &InProgress) -> bool {
    matches!(
        op.operation,
        Operation::Merge | Operation::CherryPick | Operation::Revert
    ) && !op.sequence
}

fn can_skip(op: &InProgress) -> bool {
    matches!(op.operation, Operation::Rebase | Operation::ApplyMailbox) || op.sequence
}

fn continue_message(tab: &RepoTab, op: &InProgress) -> Option<String> {
    let typed = tab.message.trim();
    if typed.is_empty() {
        op.message.clone()
    } else {
        Some(typed.to_owned())
    }
}

fn short(id: crate::model::Oid) -> String {
    id.to_hex_with_len(8).to_string()
}

fn resolve(files: &[FileDiff], resolution: Resolution) -> Command {
    let mut ours = Vec::new();
    let mut theirs = Vec::new();
    let mut remove = Vec::new();
    let mut stage = Vec::new();
    for file in files {
        let conflict = match &file.kind {
            ChangeKind::Conflicted(conflict) => Some(*conflict),
            _ => None,
        };
        match resolution {
            Resolution::Mark => stage.push(file.path.clone()),
            Resolution::Ours if conflict.is_some_and(|c| !c.ours()) => {
                remove.push(file.path.clone())
            }
            Resolution::Theirs if conflict.is_some_and(|c| !c.theirs()) => {
                remove.push(file.path.clone())
            }
            Resolution::Ours => ours.push(file.path.clone()),
            Resolution::Theirs => theirs.push(file.path.clone()),
        }
    }
    Command::Resolve {
        ours,
        theirs,
        remove,
        stage,
    }
}

fn resolve_dialog(files: &[FileDiff], resolution: Resolution) -> Option<Dialog> {
    if resolution != Resolution::Mark {
        return None;
    }
    let markers = files
        .iter()
        .filter(|file| file.conflict_markers > 0)
        .count();
    (markers > 0).then(|| Dialog::Confirm {
        text: fl!("confirm-resolved", count = markers),
        icon: icons::CIRCLE_ALERT,
        button: fl!("mark-resolved"),
        danger: false,
        command: resolve(files, resolution),
    })
}

fn conflict_menu(ui: &mut egui::Ui, merge_tool: bool) -> Option<Event> {
    let mut event = None;
    chrome::menu(
        ui,
        icons::ELLIPSIS_VERTICAL,
        fl!("conflict-actions"),
        |ui| {
            for (label, resolution) in [
                (fl!("use-ours"), Resolution::Ours),
                (fl!("use-theirs"), Resolution::Theirs),
                (fl!("mark-resolved"), Resolution::Mark),
            ] {
                if chrome::menu_item(ui, label).clicked() {
                    event = Some(Event::Resolve(resolution));
                }
            }
            if merge_tool {
                chrome::menu_separator(ui);
                if chrome::menu_item(ui, fl!("open-merge-tool")).clicked() {
                    event = Some(Event::MergeTool);
                }
            }
        },
    );
    event
}

fn conflict_note(ui: &mut egui::Ui, conflict: Conflict, markers: u32) {
    let color = theme::palette(ui.visuals()).text_muted;
    let mut texts = vec![conflict_label(conflict)];
    if markers > 0 {
        texts.push(fl!("conflict-markers", count = markers));
    }
    for text in texts {
        ui.add(Label::new(RichText::new(text).small().color(color)).selectable(false));
    }
}

fn apply(
    event: Event,
    area: Area,
    paths: &[String],
    hunks: Option<&[Hunk]>,
    run: &mut Option<Command>,
    dialog: &mut Option<Dialog>,
) {
    let paths = paths.to_vec();
    match event {
        Event::Open | Event::Resolve(_) | Event::MergeTool => {}
        Event::Stage => *run = Some(Command::Stage(paths)),
        Event::Unstage => *run = Some(Command::Unstage(paths)),
        Event::Discard => {
            *dialog = Some(Dialog::Confirm {
                text: fl!("confirm-discard", count = paths.len()),
                icon: icons::UNDO_2,
                button: fl!("discard"),
                danger: true,
                command: Command::Discard(paths),
            })
        }
        Event::Delete => {
            *dialog = Some(Dialog::Confirm {
                text: fl!("confirm-delete", count = paths.len()),
                icon: icons::TRASH_2,
                button: fl!("delete"),
                danger: true,
                command: Command::DeleteUntracked(paths),
            })
        }
        Event::Hunk(i) => {
            if let (Some(hunks), Some(path)) = (hunks, paths.first())
                && let Some(hunk) = hunks.get(i)
            {
                *run = Some(Command::ApplyPatch {
                    patch: hunk_patch(path, hunk),
                    reverse: area == Area::Staged,
                });
            }
        }
    }
}

fn area_buttons(ui: &mut egui::Ui, area: Area, all: bool) -> Option<Event> {
    if area == Area::Conflicted {
        let mut event = None;
        for (label, resolution) in [
            (fl!("use-ours"), Resolution::Ours),
            (fl!("use-theirs"), Resolution::Theirs),
            (fl!("mark-resolved"), Resolution::Mark),
        ] {
            if chrome::text_button(ui, chrome::Kind::Ghost, label).clicked() {
                event = Some(Event::Resolve(resolution));
            }
        }
        return event;
    }
    let label = |one: String, many: String| if all { many } else { one };
    let actions = match area {
        Area::Conflicted => Vec::new(),
        Area::Unstaged => vec![
            (
                icons::PLUS,
                label(fl!("stage"), fl!("stage-all")),
                Event::Stage,
            ),
            (
                icons::UNDO_2,
                label(fl!("discard"), fl!("discard-all")),
                Event::Discard,
            ),
        ],
        Area::Staged => vec![(
            icons::MINUS,
            label(fl!("unstage"), fl!("unstage-all")),
            Event::Unstage,
        )],
        Area::Untracked => vec![
            (
                icons::PLUS,
                label(fl!("stage"), fl!("stage-all")),
                Event::Stage,
            ),
            (
                icons::TRASH_2,
                label(fl!("delete"), fl!("delete-all")),
                Event::Delete,
            ),
        ],
    };
    let mut event = None;
    for (icon, label, action) in actions {
        let clicked = if all {
            chrome::button(ui, chrome::Kind::Ghost, icon, label).clicked()
        } else {
            chrome::icon(ui, icon, label).clicked()
        };
        if clicked {
            event = Some(action);
        }
    }
    event
}

fn source(area: Area) -> FileSource {
    match area {
        Area::Conflicted => FileSource::Conflicted,
        Area::Unstaged => FileSource::Unstaged,
        Area::Staged => FileSource::Staged,
        Area::Untracked => FileSource::Untracked,
    }
}

fn file_section(
    ui: &mut egui::Ui,
    tab: &mut RepoTab,
    file: &FileDiff,
    area: Option<Area>,
    source: FileSource,
    id: Id,
    default_open: bool,
) -> Option<Event> {
    let merge_tool = tab.working.as_ref().is_some_and(|w| w.merge_tool);
    let hunk_action: Option<(&'static str, String)> = match area {
        Some(Area::Unstaged) if file.kind == ChangeKind::Modified => {
            Some((icons::PLUS, fl!("stage-hunk")))
        }
        Some(Area::Staged) if file.kind == ChangeKind::Modified => {
            Some((icons::MINUS, fl!("unstage-hunk")))
        }
        _ => None,
    };
    let mut state = CollapsingState::load_with_default_open(ui.ctx(), id, default_open);
    let open = state.is_open();
    let mut event = None;
    ui.add_space(theme::space::S);
    chrome::file_card(ui, |ui| {
        let header = chrome::card_header(ui, |ui| {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if chrome::icon(ui, icons::SQUARE_ARROW_OUT_UP_RIGHT, fl!("open-full-file"))
                    .clicked()
                {
                    event = Some(Event::Open);
                }
                match area {
                    Some(Area::Conflicted) => {
                        if let Some(e) = conflict_menu(ui, merge_tool) {
                            event = Some(e);
                        }
                    }
                    Some(area) => event = area_buttons(ui, area, false).or(event.take()),
                    None => {}
                }
                stats(ui, file);
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    chrome::disclosure(ui, Some(open));
                    file_label(ui, file);
                    if let ChangeKind::Conflicted(conflict) = &file.kind {
                        conflict_note(ui, *conflict, file.conflict_markers);
                    }
                });
            });
        });
        if header.clicked() && event.is_none() {
            state.set_open(!open);
        }
        if !open {
            return;
        }
        chrome::divider(ui);
        let clicked = match &file.body {
            DiffBody::Binary => {
                chrome::empty(ui, fl!("binary-file"));
                None
            }
            DiffBody::TooLarge => {
                chrome::empty(ui, fl!("file-too-large"));
                None
            }
            DiffBody::Text(hunks) => hunks_section(ui, hunks, &hunk_action, id),
            DiffBody::Pending => match tab.hunks(source, file) {
                Some(hunks) => hunks_section(ui, &hunks, &hunk_action, id),
                None => {
                    chrome::loading(ui);
                    None
                }
            },
        };
        if let Some(i) = clicked {
            event = Some(Event::Hunk(i));
        }
    });
    state.store(ui.ctx());
    event
}

fn hunks_section(
    ui: &mut egui::Ui,
    hunks: &[Hunk],
    action: &Option<(&'static str, String)>,
    id: Id,
) -> Option<usize> {
    let mut clicked = None;
    for (i, hunk) in hunks.iter().enumerate() {
        let header = format!(
            "@@ -{},{} +{},{} @@",
            hunk.old_start, hunk.old_len, hunk.new_start, hunk.new_len
        );
        chrome::bar(ui, |ui| {
            let color = theme::palette(ui.visuals()).text_muted;
            let header = RichText::new(header).monospace().color(color);
            ui.add(Label::new(header).selectable(false));
            if let Some((icon, label)) = action {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if chrome::icon(ui, icon, label.as_str()).clicked() {
                        clicked = Some(i);
                    }
                });
            }
        });
        lines_view(ui, &hunk.lines, None, true, id.with(i));
    }
    clicked
}

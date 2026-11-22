use egui::{Label, RichText};

use super::icons;
use super::{chrome, theme};
use crate::fl;
use crate::model::{ChangeKind, Conflict, FileDiff};

#[derive(Clone, Copy, Hash, Debug)]
pub enum Toggle {
    Tree,
    All,
    Wrap,
    Split,
}

impl Toggle {
    fn id(self) -> egui::Id {
        egui::Id::NULL.with(("toggle", self))
    }

    pub fn get(self, ctx: &egui::Context) -> bool {
        ctx.data_mut(|d| *d.get_persisted_mut_or_default::<bool>(self.id()))
    }

    pub fn set(self, ctx: &egui::Context, value: bool) {
        ctx.data_mut(|d| d.insert_persisted(self.id(), value));
    }
}

pub fn kind_tint(ui: &egui::Ui, kind: &ChangeKind) -> egui::Color32 {
    let p = theme::palette(ui.visuals());
    match kind {
        ChangeKind::Added => p.added_text,
        ChangeKind::Deleted => p.removed_text,
        ChangeKind::Conflicted(_) => p.danger,
        ChangeKind::Untracked => p.text_muted,
        ChangeKind::Unchanged => p.text_faint,
        ChangeKind::Modified | ChangeKind::Renamed(_) | ChangeKind::Copied(_) => p.warn,
    }
}

pub fn conflict_label(conflict: Conflict) -> String {
    match conflict {
        Conflict::BothModified => fl!("conflict-both-modified"),
        Conflict::BothAdded => fl!("conflict-both-added"),
        Conflict::AddedByUs => fl!("conflict-added-by-us"),
        Conflict::AddedByThem => fl!("conflict-added-by-them"),
        Conflict::DeletedByUs => fl!("conflict-deleted-by-us"),
        Conflict::DeletedByThem => fl!("conflict-deleted-by-them"),
        Conflict::BothDeleted => fl!("conflict-both-deleted"),
    }
}

pub fn file_label(ui: &mut egui::Ui, file: &FileDiff) {
    let (dir, name) = match file.path.rsplit_once('/') {
        Some((dir, name)) => (Some(dir), name),
        None => (None, file.path.as_str()),
    };
    let from = match &file.kind {
        ChangeKind::Renamed(from) | ChangeKind::Copied(from) => Some(format!("← {from}")),
        _ => None,
    };
    let detail = from.or(dir.map(str::to_owned));
    let tint = kind_tint(ui, &file.kind);
    chrome::object_label(
        ui,
        kind_icon(&file.kind),
        Some(tint),
        name,
        detail.as_deref(),
    );
}

pub fn stats(ui: &mut egui::Ui, file: &FileDiff) {
    if let Some((added, removed)) = file.stats() {
        stat_pair(ui, added, removed);
    }
}

pub fn stat_pair(ui: &mut egui::Ui, added: u32, removed: u32) {
    let p = theme::palette(ui.visuals());
    let (first, second) = if ui.layout().prefer_right_to_left() {
        (
            (format!("-{removed}"), p.removed_text),
            (format!("+{added}"), p.added_text),
        )
    } else {
        (
            (format!("+{added}"), p.added_text),
            (format!("-{removed}"), p.removed_text),
        )
    };
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.x = theme::space::S;
        for (text, color) in [first, second] {
            ui.add(Label::new(RichText::new(text).monospace().color(color)).selectable(false));
        }
    });
}

pub fn kind_icon(kind: &ChangeKind) -> &'static str {
    match kind {
        ChangeKind::Added => icons::FILE_PLUS,
        ChangeKind::Deleted => icons::FILE_MINUS,
        ChangeKind::Modified | ChangeKind::Renamed(_) | ChangeKind::Copied(_) => icons::FILE_PEN,
        ChangeKind::Conflicted(_) => icons::CIRCLE_ALERT,
        ChangeKind::Untracked => icons::FILE_QUESTION_MARK,
        ChangeKind::Unchanged => icons::FILE,
    }
}

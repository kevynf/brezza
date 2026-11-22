use std::path::{Path, PathBuf};

use egui::{
    Align, CentralPanel, Id, Key, KeyboardShortcut, Label, Layout, Modal, Modifiers, Panel,
    RichText, ThemePreference,
};
use serde::{Deserialize, Serialize};

use crate::git::cli;
use crate::git::protocol::{Command, GitError, PullMode, Recovery, ResetMode};
use crate::git::worker::Job;
use crate::model::{Head, Oid, RefKind};
use crate::tab::{Dialog, NameAction, PushTarget, RepoTab, UnsavedChoice};
use crate::ui::{chrome, commits, detail, sidebar, theme};
use crate::ui::{fonts, icons};
use crate::{fl, i18n, window_icon};

const PALETTE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::P);
const OPEN: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::O);
const REFRESH: KeyboardShortcut = KeyboardShortcut::new(Modifiers::NONE, Key::F5);

#[derive(Serialize, Deserialize)]
#[serde(default)]
struct Settings {
    repos: Vec<PathBuf>,
    active: usize,
    recent: Vec<PathBuf>,
    language: Option<String>,
    sidebar: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            repos: Vec::new(),
            active: 0,
            recent: Vec::new(),
            language: None,
            sidebar: true,
        }
    }
}

#[derive(Clone)]
enum Action {
    Run(Command),
    Pull,
    Push,
    Reveal(Oid),
    Open,
    OpenPath(PathBuf),
    Refresh,
    ToggleSidebar,
    Palette,
    Language(&'static str),
    Activate(usize),
    Close(usize),
    NewTab,
    StashDialog,
    Clone,
    CloneStart { url: String, dir: PathBuf },
    NewRepo,
    Init(PathBuf),
    InitHere,
    DismissJob,
    PickParent,
    ClearRecent,
}

enum Page {
    Welcome,
    Repo(Box<RepoTab>),
    Job(Box<JobPage>),
}

impl Page {
    fn repo(&self) -> Option<&RepoTab> {
        match self {
            Page::Welcome | Page::Job(_) => None,
            Page::Repo(tab) => Some(tab),
        }
    }

    fn repo_mut(&mut self) -> Option<&mut RepoTab> {
        match self {
            Page::Welcome | Page::Job(_) => None,
            Page::Repo(tab) => Some(tab),
        }
    }
}

struct JobPage {
    title: String,
    hint: String,
    path: PathBuf,
    job: Option<Job<()>>,
    error: Option<GitError>,
}

impl JobPage {
    fn clone(ctx: &egui::Context, url: String, path: PathBuf) -> Self {
        let name = file_name(&path);
        let title = fl!("cloning", name = name.as_str());
        let hint = url.clone();
        let target = path.clone();
        let job = Job::spawn(ctx.clone(), move || cli::clone(&url, &target));
        Self {
            title,
            hint,
            path,
            job: Some(job),
            error: None,
        }
    }

    fn init(ctx: &egui::Context, path: PathBuf) -> Self {
        let name = file_name(&path);
        let title = fl!("initializing", name = name.as_str());
        let hint = path.display().to_string();
        let target = path.clone();
        let job = Job::spawn(ctx.clone(), move || cli::init(&target));
        Self {
            title,
            hint,
            path,
            job: Some(job),
            error: None,
        }
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

#[derive(Default)]
struct CloneDialog {
    url: String,
    parent: Option<PathBuf>,
    name: String,
    name_edited: bool,
}

#[derive(Default)]
struct ExitDialog {
    error: Option<GitError>,
}

enum ExitChoice {
    Save,
    Discard,
    Cancel,
}

struct Palette {
    query: String,
    selected: usize,
}

pub struct App {
    settings: Settings,
    tabs: Vec<Page>,
    palette: Option<Palette>,
    clone: Option<CloneDialog>,
    exit: Option<ExitDialog>,
    closing: bool,
    title: String,
    icon_dark: Option<bool>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::install(&cc.egui_ctx);
        egui_extras::install_image_loaders(&cc.egui_ctx);
        window_icon::use_app_icon_for_taskbar(cc);
        fonts::install(&cc.egui_ctx);
        let settings: Settings = cc
            .storage
            .and_then(|s| eframe::get_value(s, eframe::APP_KEY))
            .unwrap_or_default();
        i18n::init(settings.language.as_deref());
        let mut tabs: Vec<Page> = settings
            .repos
            .iter()
            .filter(|p| p.exists())
            .map(|p| Page::Repo(Box::new(RepoTab::open(&cc.egui_ctx, p.clone()))))
            .collect();
        if tabs.is_empty() {
            tabs.push(Page::Welcome);
        }
        let mut app = Self {
            settings,
            tabs,
            palette: None,
            clone: None,
            exit: None,
            closing: false,
            title: String::new(),
            icon_dark: None,
        };
        if let Some(path) = std::env::args_os().nth(1) {
            let path = PathBuf::from(path);
            app.apply(
                &cc.egui_ctx,
                Action::OpenPath(std::path::absolute(&path).unwrap_or(path)),
            );
        }
        app
    }

    fn repo(&mut self) -> Option<&mut RepoTab> {
        self.tabs
            .get_mut(self.settings.active)
            .and_then(Page::repo_mut)
    }

    fn dirty_files(&self) -> usize {
        self.tabs
            .iter()
            .filter_map(Page::repo)
            .map(|tab| {
                tab.open_files
                    .iter()
                    .filter(|file| file.edit.as_ref().is_some_and(|edit| edit.dirty))
                    .count()
            })
            .sum()
    }

    fn save_all_dirty(&mut self) -> Option<GitError> {
        for tab in self.tabs.iter_mut().filter_map(Page::repo_mut) {
            if !tab.has_dirty() {
                continue;
            }
            if let Err(error) = tab.save_dirty() {
                tab.error = Some(error.clone());
                return Some(error);
            }
        }
        None
    }

    fn discard_all_dirty(&mut self) {
        for tab in self.tabs.iter_mut().filter_map(Page::repo_mut) {
            for file in &mut tab.open_files {
                if file.edit.as_ref().is_some_and(|edit| edit.dirty) {
                    file.edit = None;
                }
            }
        }
    }

    fn intercept_close(&mut self, ctx: &egui::Context) {
        if !self.closing && ctx.input(|i| i.viewport().close_requested()) && self.dirty_files() > 0
        {
            let first = self.exit.is_none();
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            if first {
                let hidden = ctx.input(|i| {
                    let viewport = i.viewport();
                    viewport.visible() == Some(false) || viewport.minimized == Some(true)
                });
                if hidden {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                }
            }
            self.exit.get_or_insert_with(ExitDialog::default);
        }
    }

    fn apply(&mut self, ctx: &egui::Context, action: Action) {
        match action {
            Action::Run(command) => {
                if let Some(tab) = self.repo() {
                    tab.run(command);
                }
            }
            Action::Pull => {
                if let Some(tab) = self.repo() {
                    tab.pull();
                }
            }
            Action::Push => {
                if let Some(tab) = self.repo() {
                    tab.push();
                }
            }
            Action::Reveal(id) => {
                if let Some(tab) = self.repo() {
                    tab.reveal(id);
                }
            }
            Action::Open => {
                if let Some(path) = rfd::FileDialog::new().pick_folder() {
                    self.apply(ctx, Action::OpenPath(path));
                }
            }
            Action::OpenPath(path) => {
                let active = self.settings.active;
                let welcome = matches!(self.tabs.get(active), Some(Page::Welcome));
                let existing = self
                    .tabs
                    .iter()
                    .position(|p| p.repo().is_some_and(|t| t.path == path));
                self.settings.active = match existing {
                    Some(i) if welcome => {
                        self.tabs.remove(active);
                        if i > active { i - 1 } else { i }
                    }
                    Some(i) => i,
                    None => {
                        let page = Page::Repo(Box::new(RepoTab::open(ctx, path.clone())));
                        if welcome {
                            self.tabs[active] = page;
                            active
                        } else {
                            self.tabs.push(page);
                            self.tabs.len() - 1
                        }
                    }
                };
                self.remember(path);
            }
            Action::Refresh => {
                if let Some(tab) = self.repo() {
                    tab.refresh();
                }
            }
            Action::ToggleSidebar => self.settings.sidebar = !self.settings.sidebar,
            Action::Palette => {
                self.palette = Some(Palette {
                    query: String::new(),
                    selected: 0,
                })
            }
            Action::Language(language) => {
                i18n::set(language);
                self.settings.language = Some(language.to_owned());
            }
            Action::Activate(i) => self.settings.active = i,
            Action::Close(i) => {
                let blocked = if let Some(Page::Repo(tab)) = self.tabs.get_mut(i)
                    && tab.has_dirty()
                {
                    match tab.save_dirty() {
                        Ok(()) => false,
                        Err(error) => {
                            tab.error = Some(error);
                            true
                        }
                    }
                } else {
                    false
                };
                if !blocked {
                    self.tabs.remove(i);
                    if self.settings.active >= i && self.settings.active > 0 {
                        self.settings.active -= 1;
                    }
                    if self.tabs.is_empty() {
                        self.tabs.push(Page::Welcome);
                    }
                } else {
                    self.settings.active = i.min(self.tabs.len().saturating_sub(1));
                }
            }
            Action::NewTab => {
                self.tabs.push(Page::Welcome);
                self.settings.active = self.tabs.len() - 1;
            }
            Action::StashDialog => {
                if let Some(tab) = self.repo() {
                    tab.dialog = Some(Dialog::Stash {
                        message: String::new(),
                        include_untracked: true,
                    });
                }
            }
            Action::Clone => self.clone = Some(CloneDialog::default()),
            Action::CloneStart { url, dir } => {
                self.open_job(Page::Job(Box::new(JobPage::clone(ctx, url, dir))));
            }
            Action::NewRepo => {
                if let Some(path) = rfd::FileDialog::new().pick_folder() {
                    self.apply(ctx, Action::Init(path));
                }
            }
            Action::Init(path) => {
                self.open_job(Page::Job(Box::new(JobPage::init(ctx, path))));
            }
            Action::InitHere => {
                let path = self
                    .tabs
                    .get(self.settings.active)
                    .and_then(Page::repo)
                    .map(|tab| tab.path.clone());
                if let Some(path) = path {
                    self.tabs[self.settings.active] = Page::Job(Box::new(JobPage::init(ctx, path)));
                }
            }
            Action::DismissJob => {
                if let Some(Page::Job(page)) = self.tabs.get_mut(self.settings.active) {
                    page.error = None;
                }
            }
            Action::PickParent => {
                if let (Some(dialog), Some(path)) =
                    (&mut self.clone, rfd::FileDialog::new().pick_folder())
                {
                    dialog.parent = Some(path);
                }
            }
            Action::ClearRecent => self.settings.recent.clear(),
        }
    }

    fn remember(&mut self, path: PathBuf) {
        self.settings.recent.retain(|p| *p != path);
        self.settings.recent.insert(0, path);
        self.settings.recent.truncate(10);
    }

    fn open_job(&mut self, page: Page) {
        let active = self.settings.active;
        if matches!(self.tabs.get(active), Some(Page::Welcome)) {
            self.tabs[active] = page;
        } else {
            self.tabs.push(page);
            self.settings.active = self.tabs.len() - 1;
        }
    }

    fn poll_jobs(&mut self, ctx: &egui::Context) {
        let mut finished = Vec::new();
        for (i, page) in self.tabs.iter_mut().enumerate() {
            let Page::Job(job) = page else {
                continue;
            };
            if let Some(result) = job.job.as_ref().and_then(Job::poll) {
                finished.push((i, job.path.clone(), result));
            }
        }
        for (i, path, result) in finished {
            match result {
                Ok(()) => {
                    self.tabs[i] = Page::Repo(Box::new(RepoTab::open(ctx, path.clone())));
                    self.remember(path);
                }
                Err(error) => {
                    if let Page::Job(job) = &mut self.tabs[i] {
                        job.job = None;
                        job.error = Some(error);
                    }
                }
            }
        }
    }

    fn palette_entries(&self) -> Vec<(String, Action)> {
        let mut entries = vec![
            (fl!("open-repository"), Action::Open),
            (fl!("clone-repository"), Action::Clone),
            (fl!("new-repository"), Action::NewRepo),
            (fl!("toggle-sidebar"), Action::ToggleSidebar),
        ];
        if let Some(tab) = self.tabs.get(self.settings.active).and_then(Page::repo) {
            entries.extend([
                (fl!("refresh"), Action::Refresh),
                (fl!("fetch"), Action::Run(Command::Fetch)),
                (fl!("pull"), Action::Pull),
                (fl!("push"), Action::Push),
                (
                    fl!("stash-push"),
                    Action::Run(Command::StashPush {
                        message: None,
                        include_untracked: true,
                    }),
                ),
                (fl!("stash-pop"), Action::Run(Command::StashPop)),
            ]);
            if let Some(history) = &tab.history {
                for r in history.refs.iter().filter(|r| r.kind == RefKind::Local) {
                    entries.push((
                        format!("{}: {}", fl!("checkout"), r.name),
                        Action::Run(Command::Checkout(r.name.clone())),
                    ));
                }
            }
        }
        for (code, name) in i18n::LANGUAGES {
            entries.push((
                format!("{}: {name}", fl!("language")),
                Action::Language(code),
            ));
        }
        entries
    }

    fn top_bar(&mut self, ui: &mut egui::Ui) -> Option<Action> {
        let mut action = None;
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let languages: Vec<(&str, Option<String>)> = i18n::LANGUAGES
                .iter()
                .map(|(_, name)| (*name, None))
                .collect();
            let width = chrome::segmented_width(ui, &languages);
            chrome::popover(ui, icons::SETTINGS, fl!("settings"), width, |ui| {
                let themes = [
                    (ThemePreference::System, icons::MONITOR, fl!("theme-system")),
                    (ThemePreference::Light, icons::SUN, fl!("theme-light")),
                    (ThemePreference::Dark, icons::MOON, fl!("theme-dark")),
                ];
                let current = ui.ctx().options(|o| o.theme_preference);
                let options: Vec<(&str, Option<String>)> = themes
                    .iter()
                    .map(|(_, icon, tip)| (*icon, Some(tip.clone())))
                    .collect();
                let selected = themes.iter().position(|t| t.0 == current).unwrap_or(0);
                chrome::menu_heading(ui, fl!("theme"));
                if let Some(i) = chrome::segmented(ui, &options, selected, true) {
                    ui.ctx().set_theme(themes[i].0);
                }
                chrome::menu_separator(ui);
                let selected = i18n::LANGUAGES
                    .iter()
                    .position(|(code, _)| i18n::current() == *code)
                    .unwrap_or(0);
                chrome::menu_heading(ui, fl!("language"));
                if let Some(i) = chrome::segmented(ui, &languages, selected, true) {
                    action = Some(Action::Language(i18n::LANGUAGES[i].0));
                }
            });

            if let Some(tab) = self.tabs.get(self.settings.active).and_then(Page::repo)
                && !tab.not_repo
            {
                chrome::bar_separator(ui);
                chrome::menu(ui, icons::ARCHIVE, fl!("stash"), |ui| {
                    if chrome::menu_item(ui, fl!("stash-push")).clicked() {
                        action = Some(Action::Run(Command::StashPush {
                            message: None,
                            include_untracked: true,
                        }));
                    }
                    if chrome::menu_item(ui, fl!("stash-push-custom")).clicked() {
                        action = Some(Action::StashDialog);
                    }
                    if chrome::menu_item(ui, fl!("stash-pop")).clicked() {
                        action = Some(Action::Run(Command::StashPop));
                    }
                });
                for (icon, label, bar_action) in [
                    (icons::ARROW_UP_FROM_LINE, fl!("push"), Action::Push),
                    (icons::ARROW_DOWN_TO_LINE, fl!("pull"), Action::Pull),
                    (icons::REFRESH_CW, fl!("fetch"), Action::Run(Command::Fetch)),
                ] {
                    if chrome::icon(ui, icon, label).clicked() {
                        action = Some(bar_action);
                    }
                }
                if tab.busy() {
                    chrome::spinner(ui);
                }
                let head = tab.history.as_ref().and_then(|h| match &h.head {
                    Head::Branch(name) => h.local(name).map(|r| {
                        (
                            icons::GIT_BRANCH,
                            name.clone(),
                            r.target,
                            r.upstream.clone(),
                        )
                    }),
                    Head::Detached(id) => Some((
                        icons::GIT_COMMIT_HORIZONTAL,
                        id.to_hex_with_len(8).to_string(),
                        *id,
                        None,
                    )),
                    Head::Unborn => None,
                });
                if let Some((icon, label, id, upstream)) = head {
                    chrome::bar_separator(ui);
                    let hint = label.clone();
                    if chrome::button_truncated(
                        ui,
                        chrome::Kind::Ghost,
                        icon,
                        label,
                        theme::size::BRANCH_MAX,
                    )
                    .on_hover_text(hint)
                    .clicked()
                    {
                        action = Some(Action::Reveal(id));
                    }
                    if let Some(upstream) = upstream {
                        chrome::divergence(ui, &upstream);
                    }
                }
            }

            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                if chrome::icon(ui, icons::PANEL_LEFT, fl!("toggle-sidebar")).clicked() {
                    action = Some(Action::ToggleSidebar);
                }
                ui.add_space(theme::space::S);
                let only = self.tabs.len() == 1;
                let tabs: Vec<chrome::Tab> = self
                    .tabs
                    .iter()
                    .enumerate()
                    .map(|(i, page)| {
                        let (icon, label, hint) = match page {
                            Page::Welcome => (Some(icons::HOUSE), fl!("new-tab"), None),
                            Page::Repo(tab) => (
                                Some(icons::FOLDER_GIT_2),
                                tab.name.clone(),
                                Some(tab.path.display().to_string()),
                            ),
                            Page::Job(job) => (
                                Some(icons::FOLDER_GIT_2),
                                job.title.clone(),
                                Some(job.hint.clone()),
                            ),
                        };
                        chrome::Tab {
                            icon,
                            label,
                            hint,
                            selected: i == self.settings.active,
                            closable: !(only && matches!(page, Page::Welcome)),
                            pinned: false,
                        }
                    })
                    .collect();
                let reserve = theme::size::CONTROL + theme::space::M;
                let max = (ui.available_width() - reserve).max(0.0);
                ui.scope(|ui| {
                    ui.set_max_width(max);
                    match chrome::tabs(ui, &tabs) {
                        Some(chrome::TabAction::Select(i)) => action = Some(Action::Activate(i)),
                        Some(chrome::TabAction::Close(i)) => action = Some(Action::Close(i)),
                        None => {}
                    }
                });
                if chrome::icon(ui, icons::PLUS, fl!("new-tab")).clicked() {
                    action = Some(Action::NewTab);
                }
            });
        });
        action
    }

    fn welcome(&self, ui: &mut egui::Ui) -> Option<Action> {
        let mut action = None;
        ui.vertical_centered(|ui| {
            ui.add_space(ui.available_height() * 0.3);
            chrome::logo(ui);
            ui.add_space(theme::space::XL);
            let width = theme::size::PAGE_ACTION;
            let open = fl!("open-repository");
            if chrome::button_sized(ui, chrome::Kind::Primary, icons::FOLDER_OPEN, open, width)
                .clicked()
            {
                action = Some(Action::Open);
            }
            ui.add_space(theme::space::M);
            if chrome::text_button_sized(
                ui,
                chrome::Kind::Secondary,
                fl!("clone-repository"),
                width,
            )
            .clicked()
            {
                action = Some(Action::Clone);
            }
            ui.add_space(theme::space::S);
            if chrome::text_button_sized(ui, chrome::Kind::Secondary, fl!("new-repository"), width)
                .clicked()
            {
                action = Some(Action::NewRepo);
            }
            if self.settings.recent.is_empty() {
                return;
            }
            ui.add_space(theme::space::XL);
            let size = egui::vec2(theme::size::DIALOG, 0.0);
            ui.allocate_ui_with_layout(size, Layout::top_down(Align::Min), |ui| {
                chrome::section(ui, fl!("recent-repositories"), None, |ui| {
                    if chrome::icon(ui, icons::TRASH_2, fl!("clear-recent")).clicked() {
                        action = Some(Action::ClearRecent);
                    }
                });
                ui.spacing_mut().item_spacing.y = 0.0;
                for path in &self.settings.recent {
                    let name = path.file_name().map_or_else(
                        || path.display().to_string(),
                        |n| n.to_string_lossy().into(),
                    );
                    let full = path.display().to_string();
                    let row = chrome::list_row(ui, false, |ui| {
                        chrome::object_label(ui, icons::FOLDER_GIT_2, None, &name, Some(&full));
                    });
                    if row.clicked() {
                        action = Some(Action::OpenPath(path.clone()));
                    }
                }
            });
        });
        action
    }

    fn palette(&mut self, ctx: &egui::Context) -> Option<Action> {
        let entries = self.palette_entries();
        let palette = self.palette.as_mut()?;
        let query = palette.query.to_lowercase();
        let matches: Vec<&(String, Action)> = entries
            .iter()
            .filter(|(label, _)| label.to_lowercase().contains(&query))
            .collect();
        palette.selected = palette.selected.min(matches.len().saturating_sub(1));
        let mut chosen = None;
        let response = Modal::new(Id::unique("command-palette")).show(ctx, |ui| {
            ui.set_width(theme::size::PALETTE);
            let edit = chrome::input(ui, &mut palette.query, fl!("type-command"));
            edit.request_focus();
            ui.input_mut(|i| {
                if i.consume_key(Modifiers::NONE, Key::ArrowDown) {
                    palette.selected = (palette.selected + 1).min(matches.len().saturating_sub(1));
                }
                if i.consume_key(Modifiers::NONE, Key::ArrowUp) {
                    palette.selected = palette.selected.saturating_sub(1);
                }
                if i.consume_key(Modifiers::NONE, Key::Enter) {
                    chosen = matches.get(palette.selected).map(|(_, a)| a.clone());
                }
            });
            egui::ScrollArea::vertical()
                .max_height(theme::size::PALETTE_LIST)
                .show(ui, |ui| {
                    ui.add_space(theme::space::S);
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for (i, (label, entry)) in matches.iter().enumerate() {
                        let response = chrome::list_row(ui, i == palette.selected, |ui| {
                            ui.add(
                                egui::Label::new(label.as_str())
                                    .truncate()
                                    .selectable(false),
                            );
                        });
                        if i == palette.selected {
                            response.scroll_to_me(None);
                        }
                        if response.clicked() {
                            chosen = Some(entry.clone());
                        }
                    }
                });
        });
        if chosen.is_some() || response.should_close() {
            self.palette = None;
        }
        chosen
    }

    fn clone_dialog(&mut self, ctx: &egui::Context) -> Option<Action> {
        let dialog = self.clone.as_mut()?;
        let mut chosen = None;
        let mut pick = false;
        let mut close = false;
        let response = Modal::new(Id::unique("clone-dialog")).show(ctx, |ui| {
            let p = theme::palette(ui.visuals());
            ui.set_width(theme::size::DIALOG);
            ui.label(chrome::dialog_title(ui, fl!("clone-title")));
            ui.add_space(theme::space::M);
            ui.add(Label::new(chrome::caption(ui, fl!("clone-url"))).selectable(false));
            let _ = chrome::input(ui, &mut dialog.url, "");
            if !dialog.name_edited {
                dialog.name = url_name(&dialog.url);
            }
            ui.add_space(theme::space::M);
            ui.add(Label::new(chrome::caption(ui, fl!("clone-parent"))).selectable(false));
            ui.horizontal(|ui| {
                let choose = chrome::button(
                    ui,
                    chrome::Kind::Secondary,
                    icons::FOLDER_OPEN,
                    fl!("choose-folder"),
                );
                if choose.clicked() {
                    pick = true;
                }
                if let Some(parent) = &dialog.parent {
                    let path = parent.display().to_string();
                    ui.add(
                        Label::new(chrome::caption(ui, &path))
                            .truncate()
                            .selectable(false),
                    )
                    .on_hover_text(path);
                }
            });
            ui.add_space(theme::space::M);
            ui.add(Label::new(chrome::caption(ui, fl!("clone-name"))).selectable(false));
            if chrome::input(ui, &mut dialog.name, "").changed() {
                dialog.name_edited = true;
            }
            let name = dialog.name.trim().to_owned();
            let target = dialog.parent.as_ref().map(|parent| parent.join(&name));
            let problem = if dialog.url.trim().is_empty() {
                Some(fl!("clone-url-required"))
            } else if !name.is_empty()
                && target
                    .as_ref()
                    .is_some_and(|target| exists_non_empty(target))
            {
                Some(fl!("clone-target-exists"))
            } else {
                None
            };
            if let Some(problem) = &problem {
                ui.add_space(theme::space::S);
                ui.add(
                    Label::new(RichText::new(problem).small().color(p.danger)).selectable(false),
                );
            }
            let ready = problem.is_none() && target.is_some() && !name.is_empty();
            chrome::dialog_actions(ui, |ui| {
                let clone = ui
                    .add_enabled_ui(ready, |ui| {
                        chrome::button(ui, chrome::Kind::Primary, icons::CLOUD, fl!("clone"))
                    })
                    .inner;
                if clone.clicked() {
                    chosen = target.map(|dir| (dialog.url.trim().to_owned(), dir));
                }
                close |= cancel(ui);
            });
        });
        if chosen.is_some() || close || response.should_close() {
            self.clone = None;
        }
        if pick {
            return Some(Action::PickParent);
        }
        chosen.map(|(url, dir)| Action::CloneStart { url, dir })
    }

    fn exit_dialog(&mut self, ctx: &egui::Context) {
        let Some(dialog) = self.exit.as_ref() else {
            return;
        };
        let error = dialog.error.clone();
        let dirty = self.dirty_files();
        let mut choice = None;
        let response = Modal::new(Id::unique("exit-dialog")).show(ctx, |ui| {
            ui.set_width(theme::size::DIALOG);
            ui.label(chrome::dialog_title(ui, fl!("unsaved-changes-title")));
            ui.add_space(theme::space::M);
            ui.add(Label::new(fl!("unsaved-changes", count = dirty)).wrap());
            if let Some(error) = &error {
                let color = theme::palette(ui.visuals()).danger;
                ui.add_space(theme::space::M);
                ui.add(Label::new(RichText::new(&error.message).color(color)).wrap());
            }
            chrome::dialog_actions(ui, |ui| {
                let save = chrome::button(
                    ui,
                    chrome::Kind::Primary,
                    icons::FILE_CHECK,
                    fl!("save-all-exit"),
                );
                if save.clicked() {
                    choice = Some(ExitChoice::Save);
                }
                let discard = chrome::button(
                    ui,
                    chrome::Kind::Danger,
                    icons::TRASH_2,
                    fl!("discard-all-exit"),
                );
                if discard.clicked() {
                    choice = Some(ExitChoice::Discard);
                }
                if cancel(ui) {
                    choice = Some(ExitChoice::Cancel);
                }
            });
        });
        if choice.is_none() && response.should_close() {
            choice = Some(ExitChoice::Cancel);
        }
        match choice {
            Some(ExitChoice::Save) => match self.save_all_dirty() {
                None => self.close_window(ctx),
                Some(error) => self.exit = Some(ExitDialog { error: Some(error) }),
            },
            Some(ExitChoice::Discard) => {
                self.discard_all_dirty();
                self.close_window(ctx);
            }
            Some(ExitChoice::Cancel) => self.exit = None,
            None => {}
        }
    }

    fn close_window(&mut self, ctx: &egui::Context) {
        self.exit = None;
        self.closing = true;
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }
}

fn url_name(url: &str) -> String {
    let url = url.trim().trim_end_matches('/');
    let name = url.rsplit(['/', ':']).next().unwrap_or_default();
    name.strip_suffix(".git").unwrap_or(name).to_owned()
}

fn exists_non_empty(path: &Path) -> bool {
    path.exists() && !std::fs::read_dir(path).is_ok_and(|mut entries| entries.next().is_none())
}

fn job_page(ui: &mut egui::Ui, page: &mut JobPage) -> Option<Action> {
    let mut action = None;
    let p = theme::palette(ui.visuals());
    ui.vertical_centered(|ui| {
        ui.add_space(ui.available_height() * 0.3);
        if let Some(error) = &page.error {
            ui.label(chrome::heading(ui, &page.title));
            ui.add_space(theme::space::L);
            ui.add(Label::new(RichText::new(&error.message).color(p.danger)).wrap());
            ui.add_space(theme::space::L);
            if chrome::text_button(ui, chrome::Kind::Secondary, fl!("dismiss")).clicked() {
                action = Some(Action::DismissJob);
            }
            return;
        }
        if page.job.is_some() {
            chrome::spinner(ui);
            ui.add_space(theme::space::M);
        }
        ui.label(chrome::heading(ui, &page.title));
        ui.add_space(theme::space::S);
        ui.add(
            Label::new(RichText::new(&page.hint).small().color(p.text_muted))
                .truncate()
                .selectable(false),
        )
        .on_hover_text(page.hint.as_str());
    });
    action
}

fn missing_repo(ui: &mut egui::Ui, tab: &RepoTab) -> Option<Action> {
    let mut action = None;
    let p = theme::palette(ui.visuals());
    ui.vertical_centered(|ui| {
        ui.add_space(ui.available_height() * 0.3);
        chrome::logo(ui);
        ui.add_space(theme::space::XL);
        if chrome::button_sized(
            ui,
            chrome::Kind::Primary,
            icons::FOLDER_GIT_2,
            fl!("init-here"),
            theme::size::PAGE_ACTION,
        )
        .clicked()
        {
            action = Some(Action::InitHere);
        }
        ui.add_space(theme::space::XL);
        ui.label(chrome::heading(ui, fl!("not-a-repository")));
        ui.add_space(theme::space::S);
        let path = tab.path.display().to_string();
        ui.add(
            Label::new(RichText::new(&path).small().color(p.text_muted))
                .truncate()
                .selectable(false),
        )
        .on_hover_text(path);
    });
    action
}

fn cancel(ui: &mut egui::Ui) -> bool {
    chrome::button(ui, chrome::Kind::Secondary, icons::X, fl!("cancel")).clicked()
}

fn recover(tab: &mut RepoTab, recovery: &Recovery) {
    match recovery {
        Recovery::ForcePush { set_upstream } => {
            tab.dialog = Some(Dialog::Confirm {
                text: fl!("confirm-force-push"),
                icon: icons::ARROW_UP_FROM_LINE,
                button: fl!("force-push"),
                danger: true,
                command: Command::Push {
                    force: true,
                    set_upstream: set_upstream.clone(),
                },
            });
        }
        Recovery::ForceDeleteBranch(name) => {
            tab.dialog = Some(Dialog::Confirm {
                text: fl!("confirm-force-delete", name = name.as_str()),
                icon: icons::TRASH_2,
                button: fl!("force-delete-branch"),
                danger: true,
                command: Command::DeleteBranch {
                    name: name.clone(),
                    force: true,
                },
            });
        }
        Recovery::PullMerge => tab.run(Command::Pull(PullMode::Merge)),
        Recovery::PullRebase => tab.run(Command::Pull(PullMode::Rebase)),
    }
}

fn dialog(ctx: &egui::Context, tab: &mut RepoTab) {
    let Some(dialog) = &mut tab.dialog else {
        return;
    };
    let mut run = None;
    let mut choice = None;
    let mut reload = None;
    let mut close = false;
    let unsaved = matches!(dialog, Dialog::Unsaved { .. });
    let response = Modal::new(Id::unique("dialog")).show(ctx, |ui| {
        ui.set_width(theme::size::DIALOG);
        match dialog {
            Dialog::Name {
                title,
                value,
                action,
            } => {
                ui.label(chrome::dialog_title(ui, title.as_str()));
                ui.add_space(theme::space::M);
                chrome::input(ui, value, "").request_focus();
                let name = value.trim().to_owned();
                let (icon, button) = match action {
                    NameAction::CreateBranch(_) | NameAction::CreateTag(_) => {
                        (icons::PLUS, fl!("create"))
                    }
                    NameAction::RenameBranch(_) => (icons::GIT_BRANCH, fl!("rename")),
                };
                chrome::dialog_actions(ui, |ui| {
                    let submit = ui
                        .add_enabled_ui(!name.is_empty(), |ui| {
                            chrome::button(ui, chrome::Kind::Primary, icon, button)
                        })
                        .inner;
                    close |= cancel(ui);
                    let enter = !name.is_empty() && ui.input(|i| i.key_pressed(Key::Enter));
                    if submit.clicked() || enter {
                        run = Some(match action {
                            NameAction::CreateBranch(at) => Command::CreateBranch { name, at: *at },
                            NameAction::CreateTag(at) => Command::CreateTag { name, at: *at },
                            NameAction::RenameBranch(from) => Command::RenameBranch {
                                from: from.clone(),
                                to: name,
                            },
                        });
                    }
                });
            }
            Dialog::Remote { remotes, target } => {
                ui.label(chrome::dialog_title(ui, fl!("choose-remote")));
                ui.add_space(theme::space::M);
                ui.spacing_mut().item_spacing.y = 0.0;
                for remote in remotes.iter() {
                    let row = chrome::list_row(ui, false, |ui| {
                        chrome::object_label(ui, icons::CLOUD, None, remote, None);
                    });
                    if row.clicked() {
                        run = Some(match target {
                            PushTarget::Branch => Command::Push {
                                force: false,
                                set_upstream: Some(remote.clone()),
                            },
                            PushTarget::Tag(tag) => Command::PushTag {
                                remote: Some(remote.clone()),
                                tag: tag.clone(),
                            },
                        });
                    }
                }
                chrome::dialog_actions(ui, |ui| close |= cancel(ui));
            }
            Dialog::Autostash { command, autostash } => {
                ui.label(chrome::dialog_title(ui, fl!("autostash-hint")));
                ui.add_space(theme::space::M);
                chrome::checkbox(ui, autostash, fl!("autostash"));
                chrome::dialog_actions(ui, |ui| {
                    let continue_button = chrome::button(
                        ui,
                        chrome::Kind::Primary,
                        icons::GIT_BRANCH,
                        fl!("continue"),
                    );
                    if continue_button.clicked() {
                        run = Some(command.clone().with_autostash(*autostash));
                    }
                    close |= cancel(ui);
                });
            }
            Dialog::Reset { id, summary, mode } => {
                ui.label(chrome::dialog_title(ui, fl!("reset-to-here")));
                ui.add_space(theme::space::M);
                ui.horizontal(|ui| {
                    let hash = RichText::new(id.to_hex_with_len(10).to_string()).monospace();
                    ui.add(Label::new(hash).selectable(false));
                    ui.add(Label::new(summary.as_str()).truncate().selectable(false));
                });
                ui.add_space(theme::space::M);
                let labels = [fl!("reset-soft"), fl!("reset-mixed"), fl!("reset-hard")];
                let options: Vec<(&str, Option<String>)> =
                    labels.iter().map(|label| (label.as_str(), None)).collect();
                let selected = match mode {
                    ResetMode::Soft => 0,
                    ResetMode::Mixed => 1,
                    ResetMode::Hard => 2,
                };
                if let Some(i) = chrome::segmented(ui, &options, selected, true) {
                    *mode = [ResetMode::Soft, ResetMode::Mixed, ResetMode::Hard][i];
                }
                ui.add_space(theme::space::M);
                let (hint, danger) = match mode {
                    ResetMode::Soft => (fl!("reset-soft-hint"), false),
                    ResetMode::Mixed => (fl!("reset-mixed-hint"), false),
                    ResetMode::Hard => (fl!("reset-hard-hint"), true),
                };
                let muted = theme::palette(ui.visuals()).text_muted;
                ui.add(Label::new(RichText::new(hint).color(muted)).wrap());
                let dirty = tab.working.as_ref().is_some_and(|w| !w.is_clean());
                if danger && dirty {
                    let color = theme::palette(ui.visuals()).danger;
                    let warning = fl!("reset-hard-warning");
                    ui.add_space(theme::space::M);
                    ui.add(Label::new(RichText::new(warning).color(color)).wrap());
                }
                chrome::dialog_actions(ui, |ui| {
                    let kind = if danger {
                        chrome::Kind::Danger
                    } else {
                        chrome::Kind::Primary
                    };
                    if chrome::button(ui, kind, icons::UNDO_2, fl!("reset")).clicked() {
                        run = Some(Command::Reset {
                            to: *id,
                            mode: *mode,
                        });
                    }
                    close |= cancel(ui);
                });
            }
            Dialog::Confirm {
                text,
                icon,
                button,
                danger,
                command,
            } => {
                let p = theme::palette(ui.visuals());
                let color = if *danger { p.danger } else { p.accent_text };
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = theme::space::S;
                    ui.add(Label::new(RichText::new(*icon).color(color)).selectable(false));
                    ui.add(Label::new(chrome::dialog_title(ui, button.as_str())).selectable(false));
                });
                ui.add_space(theme::space::S);
                ui.add(Label::new(text.as_str()).wrap());
                chrome::dialog_actions(ui, |ui| {
                    let kind = if *danger {
                        chrome::Kind::Danger
                    } else {
                        chrome::Kind::Primary
                    };
                    if chrome::button(ui, kind, icon, button.as_str()).clicked() {
                        run = Some(command.clone());
                    }
                    close |= cancel(ui);
                });
            }
            Dialog::Stash {
                message,
                include_untracked,
            } => {
                ui.label(chrome::dialog_title(ui, fl!("stash-push")));
                ui.add_space(theme::space::M);
                chrome::input(ui, message, fl!("stash-message")).request_focus();
                ui.add_space(theme::space::M);
                chrome::checkbox(ui, include_untracked, fl!("stash-untracked"));
                let text = message.trim().to_owned();
                chrome::dialog_actions(ui, |ui| {
                    if chrome::button(ui, chrome::Kind::Primary, icons::ARCHIVE, fl!("stash-push"))
                        .clicked()
                    {
                        run = Some(Command::StashPush {
                            message: (!text.is_empty()).then_some(text),
                            include_untracked: *include_untracked,
                        });
                    }
                    close |= cancel(ui);
                });
            }
            Dialog::Unsaved { paths } => {
                ui.label(chrome::dialog_title(ui, fl!("unsaved-changes-title")));
                ui.add_space(theme::space::M);
                ui.label(fl!("unsaved-changes", count = paths.len()));
                chrome::dialog_actions(ui, |ui| {
                    if chrome::button(ui, chrome::Kind::Primary, icons::FILE_CHECK, fl!("save"))
                        .clicked()
                    {
                        choice = Some(UnsavedChoice::Save);
                    }
                    if chrome::button(ui, chrome::Kind::Danger, icons::TRASH_2, fl!("discard"))
                        .clicked()
                    {
                        choice = Some(UnsavedChoice::Discard);
                    }
                    close |= cancel(ui);
                });
            }
            Dialog::Reload { path } => {
                ui.label(chrome::dialog_title(ui, fl!("file-changed-on-disk")));
                ui.add_space(theme::space::M);
                ui.add(Label::new(fl!("confirm-reload")).wrap());
                chrome::dialog_actions(ui, |ui| {
                    if chrome::button(ui, chrome::Kind::Danger, icons::REFRESH_CW, fl!("reload"))
                        .clicked()
                    {
                        reload = Some(path.clone());
                    }
                    close |= cancel(ui);
                });
            }
        }
    });
    if let Some(choice) = choice {
        tab.dialog = None;
        tab.resolve_unsaved(choice);
        return;
    }
    if let Some(path) = reload {
        tab.dialog = None;
        if let Err(error) = tab.reload_file(&path) {
            tab.error = Some(error);
        }
        return;
    }
    if close || response.should_close() {
        tab.dialog = None;
        if unsaved {
            tab.resolve_unsaved(UnsavedChoice::Cancel);
        }
    }
    if let Some(command) = run {
        tab.run(command);
    }
}

impl eframe::App for App {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        self.settings.repos = self
            .tabs
            .iter()
            .filter_map(Page::repo)
            .map(|t| t.path.clone())
            .collect();
        eframe::set_value(storage, eframe::APP_KEY, &self.settings);
    }

    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.intercept_close(ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.poll_jobs(&ctx);
        for tab in self.tabs.iter_mut().filter_map(Page::repo_mut) {
            tab.poll(&ctx);
        }
        self.settings.active = self.settings.active.min(self.tabs.len().saturating_sub(1));
        self.intercept_close(&ctx);

        let mut actions = Vec::new();
        ctx.input_mut(|i| {
            if i.consume_shortcut(&PALETTE) {
                actions.push(Action::Palette);
            }
            if i.consume_shortcut(&OPEN) {
                actions.push(Action::Open);
            }
            if i.consume_shortcut(&REFRESH) {
                actions.push(Action::Refresh);
            }
        });

        let dark = ctx.theme() == egui::Theme::Dark;
        if self.icon_dark != Some(dark) {
            self.icon_dark = Some(dark);
            ctx.send_viewport_cmd(egui::ViewportCommand::Icon(Some(window_icon::mark(dark))));
        }

        let title = match self.tabs.get(self.settings.active) {
            Some(Page::Repo(tab)) => format!("{} - brezza", tab.path.display()),
            Some(Page::Job(page)) => format!("{} - brezza", page.title),
            Some(Page::Welcome) | None => "brezza".to_owned(),
        };
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }

        Panel::top("top-bar")
            .frame(chrome::top_bar(ui))
            .show_separator_line(false)
            .show(ui, |ui| {
                chrome::bar_row(ui, theme::size::TOP_BAR, |ui| {
                    actions.extend(self.top_bar(ui));
                })
            });

        let sidebar_open = self.settings.sidebar;
        let active = self.settings.active;
        match self.tabs.get_mut(active) {
            Some(Page::Repo(tab)) if tab.not_repo => {
                CentralPanel::default()
                    .frame(chrome::workspace(ui, false))
                    .show(ui, |ui| {
                        chrome::card(ui, |ui| {
                            actions.extend(missing_repo(ui, tab));
                        });
                    });
            }
            Some(Page::Repo(tab)) => {
                if let Some(error) = tab.error.clone() {
                    let actions: Vec<String> = error
                        .recovery
                        .iter()
                        .map(|recovery| match recovery {
                            Recovery::ForcePush { .. } => fl!("force-push"),
                            Recovery::ForceDeleteBranch(_) => fl!("force-delete-branch"),
                            Recovery::PullMerge => fl!("pull-merge"),
                            Recovery::PullRebase => fl!("pull-rebase"),
                        })
                        .collect();
                    Panel::bottom("error")
                        .frame(chrome::top_bar(ui))
                        .show_separator_line(false)
                        .show(ui, |ui| {
                            match chrome::notice(ui, &error.message, &actions, fl!("dismiss")) {
                                Some(chrome::NoticeAction::Dismissed) => tab.error = None,
                                Some(chrome::NoticeAction::Action(i)) => {
                                    recover(tab, &error.recovery[i]);
                                }
                                None => {}
                            }
                        });
                }
                if sidebar_open {
                    Panel::left("sidebar")
                        .resizable(true)
                        .default_size(theme::size::SIDEBAR)
                        .size_range(theme::size::SIDEBAR_MIN..=theme::size::SIDEBAR_MAX)
                        .frame(chrome::canvas(ui))
                        .show_separator_line(false)
                        .show(ui, |ui| sidebar::show(ui, tab));
                }
                CentralPanel::default()
                    .frame(chrome::workspace(ui, sidebar_open))
                    .show(ui, |ui| {
                        let max = ui.available_width() * 0.7;
                        Panel::left("commit-list")
                            .resizable(true)
                            .default_size(theme::size::LIST)
                            .size_range(theme::size::LIST_MIN..=max)
                            .frame(chrome::plain())
                            .show_separator_line(false)
                            .show(ui, |ui| {
                                chrome::card(ui, |ui| {
                                    commits::show(ui, tab);
                                });
                            });
                        CentralPanel::default()
                            .frame(chrome::gutter())
                            .show(ui, |ui| {
                                chrome::card(ui, |ui| {
                                    detail::show(ui, tab);
                                });
                            });
                    });
                dialog(&ctx, tab);
            }
            Some(Page::Job(page)) => {
                CentralPanel::default()
                    .frame(chrome::workspace(ui, false))
                    .show(ui, |ui| {
                        chrome::card(ui, |ui| {
                            actions.extend(job_page(ui, page));
                        });
                    });
            }
            Some(Page::Welcome) | None => {
                CentralPanel::default()
                    .frame(chrome::workspace(ui, false))
                    .show(ui, |ui| {
                        chrome::card(ui, |ui| {
                            actions.extend(self.welcome(ui));
                        });
                    });
            }
        }

        actions.extend(self.palette(&ctx));
        actions.extend(self.clone_dialog(&ctx));
        for action in actions {
            self.apply(&ctx, action);
        }
        if self.exit.is_some() {
            self.exit_dialog(&ctx);
        }
    }
}

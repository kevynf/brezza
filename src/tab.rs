use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use crate::git::protocol::{Command, FileSource, GitError, PullMode, Request, ResetMode, Response};
use crate::git::worker::Worker;
use crate::model::{
    CommitDetail, DiffBody, FileDiff, Head, History, Hunk, MAX_FILE, Oid, RefInfo, WorkingTree,
};
use crate::worktree::{read_file, worktree_path, write_file};

const PREVIEW_DELAY: f64 = 0.18;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Selection {
    Working,
    Commit(Oid),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ListView {
    Commits,
    Files,
}

pub enum Dialog {
    Name {
        title: String,
        value: String,
        action: NameAction,
    },
    Remote {
        remotes: Vec<String>,
        target: PushTarget,
    },
    Autostash {
        command: Command,
        autostash: bool,
    },
    Reset {
        id: Oid,
        summary: String,
        mode: ResetMode,
    },
    Confirm {
        text: String,
        icon: &'static str,
        button: String,
        danger: bool,
        command: Command,
    },
    Stash {
        message: String,
        include_untracked: bool,
    },
    Unsaved {
        paths: Vec<String>,
    },
    Reload {
        path: String,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnsavedChoice {
    Save,
    Discard,
    Cancel,
}

enum Pending {
    Select(Selection),
    CloseFile(String),
}

struct PendingCommand {
    id: u64,
    clear: Option<Vec<String>>,
}

enum CommandPolicy {
    None,
    SaveAll,
    SavePaths(Vec<String>),
    Clear(Vec<String>),
    SaveThenClear {
        save: Vec<String>,
        clear: Vec<String>,
    },
}

fn command_policy(command: &Command) -> CommandPolicy {
    match command {
        Command::Stage(paths) => CommandPolicy::SavePaths(paths.clone()),
        Command::Commit { .. } => CommandPolicy::SaveAll,
        Command::Discard(paths) | Command::DeleteUntracked(paths) => {
            CommandPolicy::Clear(paths.clone())
        }
        Command::Resolve {
            ours,
            theirs,
            remove,
            stage,
        } => {
            let mut clear = ours.clone();
            clear.extend(theirs.iter().cloned());
            clear.extend(remove.iter().cloned());
            CommandPolicy::SaveThenClear {
                save: stage.clone(),
                clear,
            }
        }
        Command::MergeTool(path) => CommandPolicy::SavePaths(vec![path.clone()]),
        Command::ApplyPatch { .. }
        | Command::Checkout(_)
        | Command::CheckoutDetached(_)
        | Command::TrackBranch { .. }
        | Command::Reset { .. }
        | Command::Merge { .. }
        | Command::Rebase { .. }
        | Command::CherryPick(_)
        | Command::Revert(_)
        | Command::Continue { .. }
        | Command::Skip(_)
        | Command::Abort(_)
        | Command::Pull(_)
        | Command::StashPush { .. }
        | Command::StashPop
        | Command::StashApply { .. }
        | Command::StashPopAt { .. } => CommandPolicy::SaveAll,
        _ => CommandPolicy::None,
    }
}

pub struct EditState {
    pub text: String,
    saved: u64,
    pub dirty: bool,
    crlf: bool,
    git_crlf: bool,
    pub generation: u64,
    preview_state: PreviewState,
    pub preview: Option<Arc<Vec<Hunk>>>,
    disk: Option<DiskState>,
    pub disk_changed: bool,
    pub preview_error: Option<GitError>,
    pub rows: HashMap<u32, usize>,
    pub last_edit: f64,
}

#[derive(Clone, Copy)]
enum PreviewState {
    Current,
    Stale,
    Sent {
        id: u64,
        generation: u64,
        source: FileSource,
    },
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DiskState {
    exists: bool,
    hash: u64,
}

impl DiskState {
    fn from_bytes(bytes: Option<&[u8]>) -> Self {
        match bytes {
            Some(bytes) => Self {
                exists: true,
                hash: hash_bytes(bytes),
            },
            None => Self {
                exists: false,
                hash: 0,
            },
        }
    }
}

impl EditState {
    fn new(text: String, hunks: Arc<Vec<Hunk>>) -> Self {
        let disk = DiskState::from_bytes(Some(text.as_bytes()));
        let (text, crlf) = normalize(text);
        let saved = hash_text(&text);
        Self {
            text,
            saved,
            dirty: false,
            crlf,
            git_crlf: hunks_crlf(&hunks),
            generation: 0,
            preview_state: PreviewState::Current,
            preview: Some(hunks),
            disk: Some(disk),
            disk_changed: false,
            preview_error: None,
            rows: HashMap::new(),
            last_edit: 0.0,
        }
    }

    fn load(&mut self, text: String, hunks: Arc<Vec<Hunk>>) {
        let disk = DiskState::from_bytes(Some(text.as_bytes()));
        let (text, crlf) = normalize(text);
        self.text = text;
        self.saved = hash_text(&self.text);
        self.dirty = false;
        self.crlf = crlf;
        self.git_crlf = hunks_crlf(&hunks);
        self.disk = Some(disk);
        self.disk_changed = false;
        self.preview_error = None;
        self.preview_state = PreviewState::Current;
        self.generation += 1;
        self.rows.clear();
        self.set_preview(hunks);
    }

    fn reload(&mut self, text: String) {
        let disk = DiskState::from_bytes(Some(text.as_bytes()));
        let (text, crlf) = normalize(text);
        self.text = text;
        self.saved = hash_text(&self.text);
        self.dirty = false;
        self.crlf = crlf;
        self.git_crlf = crlf;
        self.disk = Some(disk);
        self.disk_changed = false;
        self.preview_error = None;
        self.preview_state = PreviewState::Stale;
        self.generation += 1;
        self.preview = None;
        self.rows.clear();
        self.last_edit = 0.0;
    }

    fn invalidate_preview(&mut self) {
        self.preview_state = PreviewState::Stale;
    }

    pub fn touch(&mut self, now: f64) {
        self.dirty = hash_text(&self.text) != self.saved;
        self.generation += 1;
        if !matches!(self.preview_state, PreviewState::Sent { .. }) {
            self.preview_state = PreviewState::Stale;
        }
        self.last_edit = now;
    }

    pub fn set_saved(&mut self) {
        self.dirty = false;
        self.saved = hash_text(&self.text);
    }

    fn set_preview(&mut self, hunks: Arc<Vec<Hunk>>) {
        self.preview = Some(hunks);
        self.rows.clear();
    }

    pub fn output(&self) -> Cow<'_, str> {
        line_endings(&self.text, self.crlf)
    }

    pub fn preview_text(&self) -> Cow<'_, str> {
        line_endings(&self.text, self.git_crlf)
    }
}

fn hunks_crlf(hunks: &[Hunk]) -> bool {
    hunks
        .iter()
        .flat_map(|hunk| &hunk.lines)
        .any(|line| line.text.ends_with('\r'))
}

fn normalize(text: String) -> (String, bool) {
    if text.contains("\r\n") {
        (text.replace("\r\n", "\n"), true)
    } else {
        (text, false)
    }
}

fn line_endings(text: &str, crlf: bool) -> Cow<'_, str> {
    let normalized = if text.contains("\r\n") {
        Some(text.replace("\r\n", "\n"))
    } else {
        None
    };
    if crlf {
        Cow::Owned(normalized.as_deref().unwrap_or(text).replace('\n', "\r\n"))
    } else {
        normalized.map_or(Cow::Borrowed(text), Cow::Owned)
    }
}

fn hash_text(text: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

fn hash_bytes(bytes: &[u8]) -> u64 {
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    hasher.finish()
}

fn is_editable_source(source: FileSource) -> bool {
    matches!(
        source,
        FileSource::Conflicted | FileSource::Unstaged | FileSource::Untracked
    )
}

#[derive(Clone, Debug)]
pub enum NameAction {
    CreateBranch(Oid),
    CreateTag(Oid),
    RenameBranch(String),
}

#[derive(Clone, Debug)]
pub enum PushTarget {
    Branch,
    Tag(String),
}

pub struct OpenFile {
    pub path: String,
    pub source: FileSource,
    pub edit_source: FileSource,
    pub diff: Option<FileDiff>,
    pub edit: Option<EditState>,
    pub change: usize,
    pub jump: Option<usize>,
    request: Option<u64>,
}

pub struct RepoTab {
    pub path: PathBuf,
    pub name: String,
    worker: Worker,
    pub history: Option<Arc<History>>,
    pub working: Option<Arc<WorkingTree>>,
    pub refs_by_commit: HashMap<Oid, Vec<RefInfo>>,
    pub selection: Selection,
    pub detail: Option<CommitDetail>,
    detail_request: u64,
    pending: Vec<PendingCommand>,
    pub error: Option<GitError>,
    pub message: String,
    pub amend: bool,
    prefilled: Option<String>,
    pub filter: String,
    filtered: Option<(String, Vec<usize>)>,
    searched: String,
    match_at: Option<usize>,
    pub view: ListView,
    pub open_files: Vec<OpenFile>,
    pub active_file: Option<String>,
    pub scroll_to: Option<usize>,
    pub dialog: Option<Dialog>,
    unsaved: Option<Pending>,
    pub all_paths: Option<(Option<Oid>, Vec<String>)>,
    paths_requested: Option<Option<Oid>>,
    hunks: HashMap<u64, Arc<Vec<Hunk>>>,
    hunks_requested: HashSet<u64>,
    previews: HashSet<u64>,
    pub not_repo: bool,
}

impl RepoTab {
    pub fn open(ctx: &egui::Context, path: PathBuf) -> Self {
        let mut worker = Worker::spawn(ctx.clone(), path.clone());
        worker.send(Request::Refresh);
        Self {
            name: path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.display().to_string()),
            path,
            worker,
            history: None,
            working: None,
            refs_by_commit: HashMap::new(),
            selection: Selection::Working,
            detail: None,
            detail_request: 0,
            pending: Vec::new(),
            error: None,
            message: String::new(),
            amend: false,
            prefilled: None,
            filter: String::new(),
            filtered: None,
            searched: String::new(),
            match_at: None,
            view: ListView::Commits,
            open_files: Vec::new(),
            active_file: None,
            scroll_to: None,
            dialog: None,
            unsaved: None,
            all_paths: None,
            paths_requested: None,
            hunks: HashMap::new(),
            hunks_requested: HashSet::new(),
            previews: HashSet::new(),
            not_repo: false,
        }
    }

    pub fn poll(&mut self, ctx: &egui::Context) {
        let replies: Vec<_> = self.worker.poll().collect();
        for reply in replies {
            let id = reply.id;
            let preview = self.previews.remove(&id);
            let pending = self
                .pending
                .iter()
                .position(|pending| pending.id == id)
                .map(|index| self.pending.remove(index));
            match reply.result {
                Ok(Response::History(history)) => self.set_history(history),
                Ok(Response::Working(working)) => self.set_working(working),
                Ok(Response::Commit(detail)) if id == self.detail_request => {
                    self.detail = Some(*detail);
                    self.prune_hunks();
                }
                Ok(Response::Commit(_)) => {}
                Ok(Response::File { source, diff, text }) => {
                    self.apply_file(id, source, *diff, text);
                }
                Ok(Response::Preview {
                    source,
                    generation,
                    diff,
                }) => self.apply_preview(id, source, generation, *diff),
                Ok(Response::Hunks { version, hunks }) => {
                    self.hunks_requested.remove(&version);
                    self.hunks.insert(version, hunks);
                }
                Ok(Response::Paths { commit, paths }) => {
                    self.all_paths = Some((commit, paths));
                }
                Ok(Response::Done) => {
                    if let Some(paths) = pending.and_then(|pending| pending.clear) {
                        self.clear_edits(&paths);
                    }
                }
                Err(error) => self.fail(id, preview, error),
            }
        }
        self.schedule_previews(ctx);
    }

    fn apply_file(&mut self, id: u64, source: FileSource, diff: FileDiff, text: Option<String>) {
        let hunks = match &diff.body {
            DiffBody::Text(hunks) => Some(hunks.clone()),
            _ => None,
        };
        let editable = self.editable_source(&diff.path);
        let Some(file) = self
            .open_files
            .iter_mut()
            .find(|file| file.path == diff.path && file.source == source)
        else {
            if let Some(file) = self
                .open_files
                .iter_mut()
                .find(|file| file.request == Some(id))
            {
                file.request = None;
            }
            return;
        };
        if file.request != Some(id) {
            return;
        }
        file.request = None;
        if file.diff.is_none() {
            file.change = 0;
            file.jump = diff.change_starts().first().copied();
        }
        file.diff = Some(diff);
        if !is_editable_source(source) {
            if file.edit.as_ref().is_some_and(|edit| edit.dirty) {
                if let Some(editable) = editable
                    && file.edit_source != editable
                {
                    file.edit_source = editable;
                    if let Some(edit) = &mut file.edit {
                        edit.invalidate_preview();
                    }
                }
            } else {
                file.edit = None;
                file.edit_source = source;
            }
            return;
        }
        if file.edit.as_ref().is_some_and(|edit| edit.dirty) {
            if let Some(edit) = &mut file.edit {
                edit.disk_changed = match &text {
                    Some(text) => edit.disk != Some(DiskState::from_bytes(Some(text.as_bytes()))),
                    None => edit.disk.is_some(),
                };
            }
            if let Some(editable) = editable
                && file.edit_source != editable
            {
                file.edit_source = editable;
                if let Some(edit) = &mut file.edit {
                    edit.invalidate_preview();
                }
            }
            return;
        }
        match (text, hunks) {
            (Some(text), Some(hunks)) => {
                match file.edit.take() {
                    Some(mut edit) => {
                        edit.load(text, hunks);
                        file.edit = Some(edit);
                    }
                    None => file.edit = Some(EditState::new(text, hunks)),
                }
                file.edit_source = source;
            }
            _ => {
                file.edit = None;
                file.edit_source = source;
            }
        }
    }

    fn apply_preview(&mut self, id: u64, source: FileSource, generation: u64, diff: FileDiff) {
        let Some(file) = self.open_files.iter_mut().find(|file| {
            file.edit.as_ref().is_some_and(|edit| {
                matches!(edit.preview_state, PreviewState::Sent { id: sent, .. } if sent == id)
            })
        }) else {
            return;
        };
        let Some(edit) = &mut file.edit else {
            return;
        };
        let PreviewState::Sent {
            generation: sent,
            source: sent_source,
            ..
        } = edit.preview_state
        else {
            return;
        };
        edit.preview_state = PreviewState::Stale;
        if sent != generation || sent_source != source {
            return;
        }
        if file.path != diff.path || file.edit_source != source || !is_editable_source(source) {
            return;
        }
        if edit.generation != generation {
            return;
        }
        edit.preview_error = None;
        match diff.body {
            DiffBody::Text(hunks) => {
                edit.set_preview(hunks);
                edit.preview_state = PreviewState::Current;
            }
            DiffBody::TooLarge => {
                edit.preview_state = PreviewState::Failed;
                edit.preview_error = Some(GitError::new(crate::fl!("file-too-large")));
            }
            DiffBody::Binary | DiffBody::Pending => {
                edit.preview_state = PreviewState::Failed;
            }
        }
    }

    fn fail(&mut self, id: u64, preview: bool, error: GitError) {
        for file in &mut self.open_files {
            let Some(edit) = &mut file.edit else {
                continue;
            };
            let PreviewState::Sent {
                id: sent,
                generation,
                source,
            } = edit.preview_state
            else {
                continue;
            };
            if sent != id {
                continue;
            }
            edit.preview_state = PreviewState::Stale;
            if edit.generation == generation && file.edit_source == source {
                edit.preview_state = PreviewState::Failed;
                edit.preview_error = Some(error);
            }
            return;
        }
        if preview {
            return;
        }
        if let Some(file) = self
            .open_files
            .iter_mut()
            .find(|file| file.request == Some(id))
        {
            file.request = None;
        }
        if error.missing_repository && self.history.is_none() {
            self.not_repo = true;
            self.pending.clear();
        } else {
            self.error = Some(error);
        }
    }

    fn schedule_previews(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|input| input.time);
        let worker = &mut self.worker;
        for file in &mut self.open_files {
            let Some(edit) = &mut file.edit else {
                continue;
            };
            if !is_editable_source(file.edit_source) {
                continue;
            }
            match edit.preview_state {
                PreviewState::Sent { .. } | PreviewState::Failed => continue,
                PreviewState::Current if edit.preview.is_some() => continue,
                _ => {}
            }
            let wait = PREVIEW_DELAY - (now - edit.last_edit);
            if wait > 0.0 {
                ctx.request_repaint_after(Duration::from_secs_f64(wait));
                continue;
            }
            let id = worker.send(Request::Preview {
                source: file.edit_source,
                path: file.path.clone(),
                text: edit.preview_text().into_owned(),
                generation: edit.generation,
            });
            edit.preview_state = PreviewState::Sent {
                id,
                generation: edit.generation,
                source: file.edit_source,
            };
            self.previews.insert(id);
        }
    }

    fn set_history(&mut self, history: Arc<History>) {
        self.refs_by_commit.clear();
        for r in &history.refs {
            self.refs_by_commit
                .entry(r.target)
                .or_default()
                .push(r.clone());
        }
        self.filtered = None;
        self.history = Some(history);
    }

    fn set_working(&mut self, working: Arc<WorkingTree>) {
        let prepared = working
            .in_progress
            .as_ref()
            .and_then(|op| op.message.clone());
        let was = self
            .working
            .as_ref()
            .is_some_and(|w| w.in_progress.is_some());
        match prepared {
            Some(message) => {
                if self.message.trim().is_empty()
                    && self.prefilled.as_deref() != Some(message.as_str())
                {
                    self.message = message.clone();
                    self.prefilled = Some(message);
                }
            }
            None if was && self.prefilled.as_deref() == Some(self.message.trim()) => {
                self.message.clear();
                self.prefilled = None;
            }
            None => {}
        }
        self.working = Some(working);
        self.prune_hunks();
        if self.paths_requested == Some(None) {
            self.paths_requested = None;
        }
        if self.selection == Selection::Working {
            let paths: Vec<String> = self.open_files.iter().map(|f| f.path.clone()).collect();
            for path in paths {
                self.load_file(&path);
            }
        }
    }

    fn prune_hunks(&mut self) {
        let mut versions = HashSet::new();
        if let Some(w) = &self.working {
            for file in w
                .conflicts
                .iter()
                .chain(&w.unstaged)
                .chain(&w.staged)
                .chain(&w.untracked)
            {
                versions.insert(file.version);
            }
        }
        if let Some(d) = &self.detail {
            versions.extend(d.files.iter().map(|file| file.version));
        }
        self.hunks.retain(|version, _| versions.contains(version));
        self.hunks_requested
            .retain(|version| versions.contains(version));
    }

    pub fn hunks(&mut self, source: FileSource, file: &FileDiff) -> Option<Arc<Vec<Hunk>>> {
        if let DiffBody::Text(hunks) = &file.body {
            return Some(hunks.clone());
        }
        if let Some(hunks) = self.hunks.get(&file.version) {
            return Some(hunks.clone());
        }
        if self.hunks_requested.insert(file.version) {
            self.worker.send(Request::Hunks {
                source,
                path: file.path.clone(),
                version: file.version,
            });
        }
        None
    }

    pub fn busy(&self) -> bool {
        !self.pending.is_empty() || (self.history.is_none() && !self.not_repo)
    }

    pub fn refresh(&mut self) {
        for file in &mut self.open_files {
            if let Some(edit) = &mut file.edit
                && matches!(
                    edit.preview_state,
                    PreviewState::Sent { .. } | PreviewState::Failed
                )
            {
                edit.preview_state = PreviewState::Stale;
            }
        }
        self.worker.send(Request::Refresh);
    }

    pub fn pull(&mut self) {
        let mode = match &self.history {
            Some(history) if history.pull_configured => PullMode::Configured,
            _ => PullMode::FastForward,
        };
        self.run(Command::Pull(mode));
    }

    pub fn push(&mut self) {
        let Some(history) = &self.history else {
            return;
        };
        let branch = match &history.head {
            Head::Branch(name) => Some(name.clone()),
            _ => None,
        };
        let has_upstream = branch
            .as_deref()
            .is_some_and(|name| history.local(name).is_some_and(|r| r.upstream.is_some()));
        let remotes = history.remotes.clone();
        match branch {
            Some(_) if !has_upstream => match remotes.as_slice() {
                [] => self.run(Command::Push {
                    force: false,
                    set_upstream: None,
                }),
                [remote] => self.run(Command::Push {
                    force: false,
                    set_upstream: Some(remote.clone()),
                }),
                _ => {
                    self.dialog = Some(Dialog::Remote {
                        remotes,
                        target: PushTarget::Branch,
                    });
                }
            },
            _ => self.run(Command::Push {
                force: false,
                set_upstream: None,
            }),
        }
    }

    pub fn push_tag(&mut self, tag: &str) {
        let Some(history) = &self.history else {
            return;
        };
        let remotes = history.remotes.clone();
        match remotes.as_slice() {
            [] => self.run(Command::PushTag {
                remote: None,
                tag: tag.to_owned(),
            }),
            [remote] => self.run(Command::PushTag {
                remote: Some(remote.clone()),
                tag: tag.to_owned(),
            }),
            _ => {
                self.dialog = Some(Dialog::Remote {
                    remotes,
                    target: PushTarget::Tag(tag.to_owned()),
                });
            }
        }
    }

    pub fn integrate(&mut self, command: Command) {
        let dirty = self.working.as_ref().is_some_and(|w| !w.is_clean());
        if dirty {
            self.dialog = Some(Dialog::Autostash {
                command,
                autostash: true,
            });
        } else {
            self.run(command);
        }
    }

    pub fn run(&mut self, command: Command) {
        let policy = command_policy(&command);
        let clear = match self.apply_policy(policy) {
            Ok(clear) => clear,
            Err(error) => {
                self.error = Some(error);
                return;
            }
        };
        self.error = None;
        let id = self.worker.send(Request::Run(command));
        self.pending.push(PendingCommand { id, clear });
    }

    fn apply_policy(&mut self, policy: CommandPolicy) -> Result<Option<Vec<String>>, GitError> {
        match policy {
            CommandPolicy::None => Ok(None),
            CommandPolicy::SaveAll => {
                self.save_dirty()?;
                Ok(None)
            }
            CommandPolicy::SavePaths(paths) => {
                self.save_dirty_paths(&paths)?;
                Ok(None)
            }
            CommandPolicy::Clear(paths) => Ok(Some(paths)),
            CommandPolicy::SaveThenClear { save, clear } => {
                self.save_dirty_paths(&save)?;
                Ok(Some(clear))
            }
        }
    }

    fn save_dirty_paths(&mut self, paths: &[String]) -> Result<(), GitError> {
        let dirty = self
            .dirty_paths()
            .into_iter()
            .filter(|path| paths.contains(path))
            .collect::<Vec<_>>();
        for path in dirty {
            self.save_file(&path, false)?;
        }
        Ok(())
    }

    fn clear_edits(&mut self, paths: &[String]) {
        for file in &mut self.open_files {
            if paths.contains(&file.path) && file.edit.as_ref().is_some_and(|edit| edit.dirty) {
                file.edit = None;
            }
        }
    }

    pub fn has_dirty(&self) -> bool {
        self.open_files
            .iter()
            .any(|file| file.edit.as_ref().is_some_and(|edit| edit.dirty))
    }

    fn dirty_paths(&self) -> Vec<String> {
        self.open_files
            .iter()
            .filter(|file| file.edit.as_ref().is_some_and(|edit| edit.dirty))
            .map(|file| file.path.clone())
            .collect()
    }

    pub fn save_dirty(&mut self) -> Result<(), GitError> {
        let paths = self.dirty_paths();
        for path in paths {
            self.save_file(&path, false)?;
        }
        Ok(())
    }

    pub fn save_file(&mut self, path: &str, overwrite: bool) -> Result<(), GitError> {
        let Some(index) = self.open_files.iter().position(|file| file.path == path) else {
            return Err(GitError::new(path.to_owned()));
        };
        let dirty = self.open_files[index]
            .edit
            .as_ref()
            .is_some_and(|edit| edit.dirty);
        if !dirty {
            return Ok(());
        }
        let root = self
            .working
            .as_ref()
            .map(|working| working.root.clone())
            .ok_or_else(|| GitError::new(path.to_owned()))?;
        let full = worktree_path(&root, path)?;
        let oversized = std::fs::metadata(&full).is_ok_and(|meta| meta.len() > MAX_FILE);
        let current = if oversized {
            None
        } else {
            read_file(&full).map_err(|error| GitError::new(format!("{path}: {error}")))?
        };
        let state = DiskState::from_bytes(current.as_deref());
        let Some(edit) = &mut self.open_files[index].edit else {
            return Ok(());
        };
        if edit.disk != Some(state) && !overwrite {
            edit.disk_changed = true;
            return Err(GitError::new(format!(
                "{path}: {}",
                crate::fl!("file-changed-on-disk")
            )));
        }
        let output = edit.output();
        let disk = DiskState::from_bytes(Some(output.as_bytes()));
        write_file(&full, output.as_bytes())
            .map_err(|error| GitError::new(format!("{path}: {error}")))?;
        edit.set_saved();
        edit.disk = Some(disk);
        edit.disk_changed = false;
        edit.preview_error = None;
        if matches!(edit.preview_state, PreviewState::Failed) {
            edit.preview_state = PreviewState::Stale;
        }
        self.open_files[index].request = None;
        self.worker.send(Request::Refresh);
        Ok(())
    }

    pub fn reload_file(&mut self, path: &str) -> Result<(), GitError> {
        let root = self
            .working
            .as_ref()
            .map(|working| working.root.clone())
            .ok_or_else(|| GitError::new(path.to_owned()))?;
        let full = worktree_path(&root, path)?;
        let size = std::fs::metadata(&full)
            .map_err(|error| GitError::new(format!("{path}: {error}")))?
            .len();
        if size > MAX_FILE {
            return Err(GitError::new(format!(
                "{path}: {}",
                crate::fl!("file-too-large")
            )));
        }
        let bytes =
            std::fs::read(&full).map_err(|error| GitError::new(format!("{path}: {error}")))?;
        if bytes.len() as u64 > MAX_FILE {
            return Err(GitError::new(format!(
                "{path}: {}",
                crate::fl!("file-too-large")
            )));
        }
        let text = String::from_utf8(bytes)
            .map_err(|_| GitError::new(format!("{path}: invalid UTF-8")))?;
        let Some(file) = self.open_files.iter_mut().find(|file| file.path == path) else {
            return Err(GitError::new(path.to_owned()));
        };
        let Some(edit) = &mut file.edit else {
            return Err(GitError::new(path.to_owned()));
        };
        edit.reload(text);
        file.request = None;
        self.worker.send(Request::Refresh);
        Ok(())
    }

    fn discard_dirty(&mut self) {
        let paths = self.dirty_paths();
        for path in &paths {
            if let Some(file) = self.open_files.iter_mut().find(|file| file.path == *path) {
                file.edit = None;
            }
        }
        for path in paths {
            self.load_file(&path);
        }
    }

    pub fn resolve_unsaved(&mut self, choice: UnsavedChoice) {
        match choice {
            UnsavedChoice::Cancel => {
                self.unsaved = None;
                return;
            }
            UnsavedChoice::Save => {
                if let Err(error) = self.save_dirty() {
                    self.error = Some(error);
                    self.dialog = Some(Dialog::Unsaved {
                        paths: self.dirty_paths(),
                    });
                    return;
                }
            }
            UnsavedChoice::Discard => self.discard_dirty(),
        }
        match self.unsaved.take() {
            Some(Pending::Select(selection)) => self.select_now(selection),
            Some(Pending::CloseFile(path)) => self.close_now(&path),
            None => {}
        }
    }

    pub fn select(&mut self, selection: Selection) {
        if self.selection == selection {
            return;
        }
        let paths = self.dirty_paths();
        if !paths.is_empty() {
            self.unsaved = Some(Pending::Select(selection));
            self.dialog = Some(Dialog::Unsaved { paths });
            return;
        }
        self.select_now(selection);
    }

    fn select_now(&mut self, selection: Selection) {
        if self.selection != selection {
            self.open_files.clear();
            self.active_file = None;
        }
        self.selection = selection;
        if let Selection::Commit(id) = selection
            && self.detail.as_ref().is_none_or(|d| d.id != id)
        {
            self.detail = None;
            self.detail_request = self.worker.send(Request::Commit(id));
        }
    }

    fn source_for(&self, path: &str) -> FileSource {
        match self.selection {
            Selection::Commit(id) => FileSource::Commit(id),
            Selection::Working => {
                let w = self.working.as_deref();
                let has = |files: Option<&Vec<FileDiff>>| {
                    files.is_some_and(|files| files.iter().any(|f| f.path == path))
                };
                if has(w.map(|w| &w.conflicts)) {
                    FileSource::Conflicted
                } else if has(w.map(|w| &w.unstaged)) {
                    FileSource::Unstaged
                } else if has(w.map(|w| &w.staged)) {
                    FileSource::Staged
                } else if has(w.map(|w| &w.untracked)) {
                    FileSource::Untracked
                } else {
                    FileSource::Unstaged
                }
            }
        }
    }

    fn editable_source(&self, path: &str) -> Option<FileSource> {
        if self.selection != Selection::Working {
            return None;
        }
        let working = self.working.as_deref()?;
        if working.conflicts.iter().any(|file| file.path == path) {
            Some(FileSource::Conflicted)
        } else if working.untracked.iter().any(|file| file.path == path) {
            Some(FileSource::Untracked)
        } else if working
            .unstaged
            .iter()
            .chain(&working.staged)
            .any(|file| file.path == path)
        {
            Some(FileSource::Unstaged)
        } else {
            None
        }
    }

    fn load_file(&mut self, path: &str) {
        let source = self.source_for(path);
        let editable = self.editable_source(path);
        let id = self.worker.send(Request::File {
            source,
            path: path.to_owned(),
        });
        if let Some(file) = self.open_files.iter_mut().find(|file| file.path == path) {
            if file.source != source {
                file.diff = None;
            }
            file.source = source;
            let dirty = file.edit.as_ref().is_some_and(|edit| edit.dirty);
            if !dirty && !is_editable_source(source) {
                file.edit = None;
                file.edit_source = source;
                file.request = Some(id);
                return;
            }
            let edit_source = if dirty {
                editable.unwrap_or(file.edit_source)
            } else {
                source
            };
            file.edit_source = edit_source;
            if dirty && let Some(edit) = &mut file.edit {
                edit.invalidate_preview();
            }
            file.request = Some(id);
        }
    }

    pub fn open_file(&mut self, path: &str) {
        if !self.open_files.iter().any(|f| f.path == path) {
            let source = self.source_for(path);
            self.open_files.push(OpenFile {
                path: path.to_owned(),
                source,
                edit_source: source,
                diff: None,
                edit: None,
                change: 0,
                jump: None,
                request: None,
            });
            self.load_file(path);
        }
        self.active_file = Some(path.to_owned());
    }

    pub fn request_paths(&mut self) {
        let commit = match self.selection {
            Selection::Working => None,
            Selection::Commit(id) => Some(id),
        };
        if self.paths_requested != Some(commit) {
            self.paths_requested = Some(commit);
            self.worker.send(Request::Paths(commit));
        }
    }

    pub fn paths(&self) -> Option<&[String]> {
        let commit = match self.selection {
            Selection::Working => None,
            Selection::Commit(id) => Some(id),
        };
        self.all_paths
            .as_ref()
            .filter(|(c, _)| *c == commit)
            .map(|(_, paths)| paths.as_slice())
    }

    pub fn close_file(&mut self, path: &str) {
        if self
            .open_files
            .iter()
            .find(|file| file.path == path)
            .and_then(|file| file.edit.as_ref())
            .is_some_and(|edit| edit.dirty)
        {
            self.unsaved = Some(Pending::CloseFile(path.to_owned()));
            self.dialog = Some(Dialog::Unsaved {
                paths: vec![path.to_owned()],
            });
            return;
        }
        self.close_now(path);
    }

    fn close_now(&mut self, path: &str) {
        self.open_files.retain(|f| f.path != path);
        if self.active_file.as_deref() == Some(path) {
            self.active_file = None;
        }
    }

    pub fn reveal(&mut self, id: Oid) {
        self.filter.clear();
        self.select(Selection::Commit(id));
        if let Some(history) = &self.history {
            self.scroll_to = history
                .commits
                .iter()
                .position(|c| c.id == id)
                .map(|i| i + 1);
        }
    }

    pub fn head_branch(&self) -> Option<&str> {
        match &self.history.as_ref()?.head {
            Head::Branch(name) => Some(name),
            _ => None,
        }
    }

    pub fn visible_commits(&mut self) -> Option<&[usize]> {
        let history = self.history.as_ref()?;
        let query = self.filter.trim().to_lowercase();
        if query.is_empty() {
            return None;
        }
        if self.filtered.as_ref().is_none_or(|(q, _)| *q != query) {
            let hits = history
                .commits
                .iter()
                .enumerate()
                .filter(|(_, c)| {
                    c.summary.to_lowercase().contains(&query)
                        || c.author.to_lowercase().contains(&query)
                        || c.id.to_string().starts_with(&query)
                })
                .map(|(i, _)| i)
                .collect();
            self.filtered = Some((query, hits));
        }
        let (query, _) = self.filtered.as_ref()?;
        if *query != self.searched {
            self.searched = query.clone();
            self.match_at = None;
            self.scroll_to = Some(0);
        }
        self.filtered.as_ref().map(|(_, hits)| hits.as_slice())
    }

    pub fn match_position(&mut self) -> Option<(Option<usize>, usize)> {
        let total = self.visible_commits()?.len();
        Some((self.match_at.map(|m| m + 1), total))
    }

    pub fn next_match(&mut self, step: isize) {
        let Some(hits) = self.visible_commits().map(<[usize]>::to_vec) else {
            return;
        };
        if hits.is_empty() {
            return;
        }
        let len = hits.len() as isize;
        let at = self
            .match_at
            .map_or(if step < 0 { len - 1 } else { 0 }, |m| {
                (m as isize + step).rem_euclid(len)
            });
        self.match_at = Some(at as usize);
        let index = hits[at as usize];
        if let Some(id) = self.history.as_ref().map(|h| h.commits[index].id) {
            self.select(Selection::Commit(id));
        }
        self.scroll_to = Some(at as usize + 1);
    }
}

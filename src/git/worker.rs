use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use notify::{EventKind, RecursiveMode, Watcher};

use super::cli;
use super::protocol::{Command, FileSource, GitError, Request, Response};
use super::repo::{Excludes, Repo};

const DEBOUNCE: Duration = Duration::from_millis(150);

pub struct Reply {
    pub id: u64,
    pub result: Result<Response, GitError>,
}

pub struct Job<T> {
    rx: Receiver<Result<T, GitError>>,
}

impl<T: Send + 'static> Job<T> {
    pub fn spawn(
        ctx: egui::Context,
        work: impl FnOnce() -> Result<T, GitError> + Send + 'static,
    ) -> Self {
        let (tx, rx) = mpsc::channel();
        thread::Builder::new()
            .name("brezza-git-job".to_owned())
            .spawn(move || {
                let result = match panic::catch_unwind(AssertUnwindSafe(work)) {
                    Ok(result) => result,
                    Err(payload) => Err(internal(payload)),
                };
                let _ = tx.send(result);
                ctx.request_repaint();
            })
            .expect("spawn git job");
        Self { rx }
    }

    pub fn poll(&self) -> Option<Result<T, GitError>> {
        self.rx.try_recv().ok()
    }
}

pub struct Worker {
    tx: Sender<Message>,
    writer: Sender<(u64, Command)>,
    rx: Receiver<Reply>,
    next_id: u64,
    _watchers: Arc<Mutex<Vec<notify::RecommendedWatcher>>>,
}

enum Message {
    Request(u64, Request),
    Change(Vec<PathBuf>),
    Done,
}

#[derive(Default)]
struct Refresh {
    ids: Vec<u64>,
    history: bool,
    working: bool,
    reopen: bool,
}

impl Refresh {
    fn all(&mut self) {
        self.history = true;
        self.working = true;
    }

    fn explicit(&mut self, id: u64) {
        self.ids.push(id);
        self.all();
        self.reopen = true;
    }

    fn change(&mut self, repo: &Repo, paths: Vec<PathBuf>) {
        let mut paths = paths;
        paths.sort_unstable();
        paths.dedup();
        let mut worktree = Vec::new();
        for path in paths {
            if let Some(relative) = git_path(repo, &path) {
                self.git_path(&relative);
            } else if let Some(root) = repo.workdir()
                && let Ok(relative) = path.strip_prefix(root)
            {
                worktree.push((relative.to_owned(), path.is_dir()));
            }
        }
        if worktree.is_empty() {
            return;
        }
        let mut excludes = repo.excludes().ok();
        for (path, dir) in worktree {
            if !excluded(&mut excludes, &path, dir) {
                self.working = true;
                break;
            }
        }
    }

    fn git_path(&mut self, path: &Path) {
        if path
            .extension()
            .is_some_and(|extension| extension == "lock")
        {
            return;
        }
        let stash = Path::new("logs").join("refs").join("stash");
        let head = path
            .file_name()
            .is_some_and(|name| name.to_str().is_some_and(|name| name.ends_with("_HEAD")));
        if path == Path::new("config") {
            self.reopen = true;
            self.all();
        } else if path == Path::new("HEAD")
            || path == Path::new("packed-refs")
            || path == Path::new("MERGE_MSG")
            || path == stash
            || path.starts_with("refs")
            || path.starts_with("rebase-merge")
            || path.starts_with("rebase-apply")
            || path.starts_with("sequencer")
            || head
        {
            self.all();
        } else if path == Path::new("index") {
            self.working = true;
        }
    }
}

fn git_path(repo: &Repo, path: &Path) -> Option<PathBuf> {
    [repo.git_dir(), repo.common_dir()]
        .into_iter()
        .find_map(|dir| path.strip_prefix(dir).ok().map(Path::to_owned))
}

fn excluded(excludes: &mut Option<Excludes<'_>>, path: &Path, dir: bool) -> bool {
    let Some(excludes) = excludes else {
        return false;
    };
    let mut parent = path.parent();
    while let Some(dir) = parent {
        if !dir.as_os_str().is_empty() && excludes.is_excluded(dir, true) {
            return true;
        }
        parent = dir.parent();
    }
    excludes.is_excluded(path, dir)
}

impl Worker {
    pub fn spawn(ctx: egui::Context, root: PathBuf) -> Self {
        let (tx, messages) = mpsc::channel();
        let (writer, commands) = mpsc::channel();
        let (replies, rx) = mpsc::channel();
        {
            let ctx = ctx.clone();
            let reader = tx.clone();
            let root = root.clone();
            let replies = replies.clone();
            thread::Builder::new()
                .name("brezza-git-write".to_owned())
                .spawn(move || write(root, commands, reader, replies, ctx))
                .expect("spawn git writer");
        }
        let events = tx.clone();
        let watchers = Arc::new(Mutex::new(Vec::new()));
        let slot = watchers.clone();
        thread::Builder::new()
            .name("brezza-git".to_owned())
            .spawn(move || read(root, messages, events, slot, replies, ctx))
            .expect("spawn git reader");
        Self {
            tx,
            writer,
            rx,
            next_id: 0,
            _watchers: watchers,
        }
    }

    pub fn send(&mut self, request: Request) -> u64 {
        self.next_id += 1;
        match request {
            Request::Run(command) => {
                let _ = self.writer.send((self.next_id, command));
            }
            request => {
                let _ = self.tx.send(Message::Request(self.next_id, request));
            }
        }
        self.next_id
    }

    pub fn poll(&self) -> impl Iterator<Item = Reply> + '_ {
        self.rx.try_iter()
    }
}

fn watch(roots: &[PathBuf], tx: Sender<Message>) -> Vec<notify::RecommendedWatcher> {
    let mut watchers = Vec::new();
    for root in roots {
        let tx = tx.clone();
        let Ok(mut watcher) =
            notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
                let Ok(event) = event else {
                    let _ = tx.send(Message::Done);
                    return;
                };
                if !matches!(event.kind, EventKind::Access(_)) {
                    let _ = tx.send(Message::Change(event.paths));
                }
            })
        else {
            continue;
        };
        if watcher.watch(root, RecursiveMode::Recursive).is_ok() {
            watchers.push(watcher);
        }
    }
    watchers
}

fn write(
    root: PathBuf,
    commands: Receiver<(u64, Command)>,
    reader: Sender<Message>,
    replies: Sender<Reply>,
    ctx: egui::Context,
) {
    while let Ok((id, command)) = commands.recv() {
        let result = match panic::catch_unwind(AssertUnwindSafe(|| cli::run(&root, &command))) {
            Ok(Ok(())) => Ok(Response::Done),
            Ok(Err(error)) => Err(error),
            Err(payload) => Err(internal(payload)),
        };
        let _ = replies.send(Reply { id, result });
        ctx.request_repaint();
        let _ = reader.send(Message::Done);
    }
}

fn read(
    root: PathBuf,
    messages: Receiver<Message>,
    events: Sender<Message>,
    watchers: Arc<Mutex<Vec<notify::RecommendedWatcher>>>,
    replies: Sender<Reply>,
    ctx: egui::Context,
) {
    let reply = |id, result| {
        let _ = replies.send(Reply { id, result });
        ctx.request_repaint();
    };
    let mut repo = match Repo::open(&root) {
        Ok(repo) => repo,
        Err(e) => return reply(0, Err(e)),
    };
    *watchers.lock().expect("watch slot") = watch(&repo.watch_roots(), events);
    let mut config = repo.config_mtime();
    while let Ok(first) = messages.recv() {
        let mut refresh = Refresh::default();
        let mut requests = Vec::new();
        let mut changes = Vec::new();
        if matches!(first, Message::Change(_)) {
            thread::sleep(DEBOUNCE);
        }
        for message in std::iter::once(first).chain(messages.try_iter()) {
            match message {
                Message::Request(id, Request::Refresh) => refresh.explicit(id),
                Message::Request(id, request) => requests.push((id, request)),
                Message::Change(paths) => changes.extend(paths),
                Message::Done => refresh.all(),
            }
        }
        if !changes.is_empty() {
            let result = guarded(&mut repo, |repo| {
                refresh.change(repo, changes);
                Ok(())
            });
            if let Err(error) = result {
                reply(0, Err(error));
                refresh.all();
            }
        }
        for (id, request) in requests {
            let result = match request {
                Request::Refresh => continue,
                Request::Commit(oid) => guarded(&mut repo, |repo| repo.commit(oid))
                    .map(|c| Response::Commit(Box::new(c))),
                Request::File { source, path } => guarded(&mut repo, |repo| {
                    let diff = repo.file(source, &path)?;
                    let text = match source {
                        FileSource::Staged | FileSource::Commit(_) => None,
                        FileSource::Conflicted | FileSource::Unstaged | FileSource::Untracked => {
                            repo.worktree_text(&path)
                        }
                    };
                    Ok(Response::File {
                        source,
                        diff: Box::new(diff),
                        text,
                    })
                }),
                Request::Preview {
                    source,
                    path,
                    text,
                    generation,
                } => guarded(&mut repo, |repo| repo.preview(source, &path, &text)).map(|diff| {
                    Response::Preview {
                        source,
                        generation,
                        diff: Box::new(diff),
                    }
                }),
                Request::Hunks {
                    source,
                    path,
                    version,
                } => guarded(&mut repo, |repo| repo.hunks(source, &path, version))
                    .map(|hunks| Response::Hunks { version, hunks }),
                Request::Paths(commit) => guarded(&mut repo, |repo| repo.paths(commit))
                    .map(|paths| Response::Paths { commit, paths }),
                Request::Run(_) => continue,
            };
            reply(id, result);
        }
        if repo.config_mtime() != config {
            config = repo.config_mtime();
            refresh.all();
            refresh.reopen = true;
        }
        if refresh.reopen
            && let Err(error) = repo.reload()
        {
            if refresh.ids.is_empty() {
                reply(0, Err(error));
            } else {
                for id in refresh.ids {
                    reply(id, Err(error.clone()));
                }
            }
            continue;
        }
        if !refresh.history && !refresh.working {
            continue;
        }
        let force = refresh.reopen;
        let mut results = Vec::new();
        if refresh.history {
            match guarded(&mut repo, |repo| repo.history(force)) {
                Ok(Some(history)) => results.push(Ok(Response::History(history))),
                Ok(None) => {}
                Err(error) => results.push(Err(error)),
            }
        }
        if refresh.working {
            results.push(guarded(&mut repo, |repo| repo.working()).map(Response::Working));
        }
        if refresh.ids.is_empty() {
            for result in results {
                reply(0, result);
            }
        } else {
            for id in refresh.ids {
                for result in &results {
                    reply(id, result.clone());
                }
            }
        }
    }
}

fn guarded<T>(
    repo: &mut Repo,
    f: impl FnOnce(&mut Repo) -> Result<T, GitError>,
) -> Result<T, GitError> {
    match panic::catch_unwind(AssertUnwindSafe(|| f(repo))) {
        Ok(result) => result,
        Err(payload) => {
            let _ = repo.reload();
            Err(internal(payload))
        }
    }
}

fn internal(payload: Box<dyn std::any::Any + Send>) -> GitError {
    let message = payload
        .downcast_ref::<&str>()
        .map(|s| (*s).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_default();
    GitError::new(format!("internal error: {message}"))
}

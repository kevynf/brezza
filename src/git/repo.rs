use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use gix::bstr::{BString, ByteSlice};
use gix::diff::index::ChangeRef;
use gix::index::entry::Stage;
use gix::object::tree::diff::ChangeDetached;
use gix::refs::Category;
use gix::revision::walk::Sorting;
use gix::status::index_worktree::iter::Summary;
use gix::status::{UntrackedFiles, index_worktree};

use super::protocol::{FileSource, GitError};
use crate::diff::{CONTEXT, Cache, FULL, Stats, measure, summarize};
use crate::graph;
use crate::model::{
    ChangeKind, CommitDetail, CommitSummary, Conflict, DIVERGENCE_LIMIT, DiffBody, FOLD_FILES,
    FileDiff, Head, History, Hunk, InProgress, LAZY_LIMIT, MAX_FILE, Oid, Operation, RefInfo,
    RefKind, Signature, StashInfo, Upstream, WorkingTree, default_open,
};

const LOG_LIMIT: usize = 20_000;

type HistoryKey = (Head, Vec<(String, RefKind, Oid)>, Vec<Oid>);
type CountKey = (Oid, Oid);

enum Content {
    Blob(Option<Oid>),
    Worktree,
    Lazy(u64),
    Data(Option<Vec<u8>>),
}

type Blobs = (Vec<u8>, Vec<u8>);

struct Resolved {
    path: String,
    kind: ChangeKind,
    old: Option<Oid>,
    new: Content,
    hash: gix::hash::Kind,
    id: Option<Oid>,
}

impl Resolved {
    fn new(
        hash: gix::hash::Kind,
        path: String,
        kind: ChangeKind,
        old: Option<Oid>,
        new: Content,
    ) -> Self {
        let mut resolved = Self {
            path,
            kind,
            old,
            new,
            hash,
            id: None,
        };
        resolved.seal();
        resolved
    }

    fn replace(&mut self, new: Content) {
        self.new = new;
        self.seal();
    }

    fn seal(&mut self) {
        self.id = match &self.new {
            Content::Blob(id) => *id,
            Content::Data(Some(data)) => {
                gix::objs::compute_hash(self.hash, gix::objs::Kind::Blob, data).ok()
            }
            Content::Worktree | Content::Lazy(_) | Content::Data(None) => None,
        };
        if self.kind == ChangeKind::Modified && self.old.is_some() && self.old == self.id {
            self.kind = ChangeKind::Unchanged;
        }
    }

    fn version(&self) -> u64 {
        let mut hash = 0xcbf2_9ce4_8422_2325;
        for &byte in self.path.as_bytes() {
            hash = mix(hash, u64::from(byte));
        }
        hash = mix(hash, kind_code(&self.kind));
        if let ChangeKind::Renamed(from) | ChangeKind::Copied(from) = &self.kind {
            for &byte in from.as_bytes() {
                hash = mix(hash, u64::from(byte));
            }
        }
        if let Content::Lazy(stamp) = &self.new {
            hash = mix(hash, *stamp);
        }
        for id in [self.old, self.id] {
            hash = mix(hash, u64::from(id.is_some()));
            if let Some(id) = id {
                for &byte in id.as_bytes() {
                    hash = mix(hash, u64::from(byte));
                }
            }
        }
        hash
    }

    fn markers(&self) -> u32 {
        match (&self.kind, &self.new) {
            (ChangeKind::Conflicted(_), Content::Data(Some(data))) => {
                data.split(|&byte| byte == b'\n')
                    .filter(|line| line.starts_with(b"<<<<<<<"))
                    .count() as u32
            }
            _ => 0,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
    Unstaged,
    Staged,
    Untracked,
}

pub struct Excludes<'repo> {
    stack: gix::AttributeStack<'repo>,
}

impl Excludes<'_> {
    pub fn is_excluded(&mut self, path: &Path, dir: bool) -> bool {
        let mode = dir.then_some(gix::index::entry::Mode::DIR);
        self.stack
            .at_path(path, mode)
            .is_ok_and(|platform| platform.is_excluded())
    }
}

pub struct Repo {
    repo: gix::Repository,
    cache: HashMap<Oid, CommitSummary>,
    counts: HashMap<CountKey, (u32, u32)>,
    key: Option<HistoryKey>,
    diffs: Cache,
}

impl Repo {
    pub fn open(path: &Path) -> Result<Self, GitError> {
        let mut repo = gix::discover(path).map_err(open_error)?;
        repo.object_cache_size_if_unset(64 << 20);
        let diffs = Cache::new(repo.object_hash());
        Ok(Self {
            repo,
            cache: HashMap::new(),
            counts: HashMap::new(),
            key: None,
            diffs,
        })
    }

    pub fn root(&self) -> PathBuf {
        self.repo
            .workdir()
            .unwrap_or(self.repo.git_dir())
            .to_owned()
    }

    pub fn git_dir(&self) -> &Path {
        self.repo.git_dir()
    }

    pub fn common_dir(&self) -> &Path {
        self.repo.common_dir()
    }

    pub fn workdir(&self) -> Option<&Path> {
        self.repo.workdir()
    }

    pub fn excludes(&self) -> Result<Excludes<'_>, GitError> {
        let index = self.repo.index_or_empty()?;
        let stack = self.repo.excludes(
            &index,
            None,
            gix::worktree::stack::state::ignore::Source::WorktreeThenIdMappingIfNotSkipped,
        )?;
        Ok(Excludes { stack })
    }

    pub fn reload(&mut self) -> Result<(), GitError> {
        let fresh = Self::open(&self.root())?;
        self.repo = fresh.repo;
        self.counts.clear();
        self.key = None;
        Ok(())
    }

    pub fn watch_roots(&self) -> Vec<PathBuf> {
        let root = self.root();
        let mut roots = vec![root.clone()];
        for dir in [self.repo.git_dir(), self.repo.common_dir()] {
            if !dir.starts_with(&root) && !roots.iter().any(|r| r == dir) {
                roots.push(dir.to_owned());
            }
        }
        roots
    }

    pub fn config_mtime(&self) -> Option<std::time::SystemTime> {
        self.repo
            .common_dir()
            .join("config")
            .metadata()
            .and_then(|m| m.modified())
            .ok()
    }

    pub fn history(&mut self, force: bool) -> Result<Option<Arc<History>>, GitError> {
        let head = self.head()?;
        let refs = self.refs()?;
        let stashes = self.stashes()?;
        let key: HistoryKey = (
            head.clone(),
            refs.iter()
                .map(|r| (r.name.clone(), r.kind, r.target))
                .collect(),
            stashes.iter().map(|stash| stash.id).collect(),
        );
        if !force && self.key.as_ref() == Some(&key) {
            return Ok(None);
        }
        let head_id = self.repo.head_id().ok().map(|id| id.detach());
        let mut tips: Vec<Oid> = refs.iter().map(|r| r.target).chain(head_id).collect();
        tips.retain(|id| self.repo.find_commit(*id).is_ok());
        let commits = self.log(tips)?;
        let graph = graph::layout(&commits, head_id);
        let lanes = graph::lanes(&graph);
        let remotes = self
            .repo
            .remote_names()
            .into_iter()
            .map(|name| name.to_string())
            .collect();
        let pull_configured = self.pull_configured(&head);
        self.key = Some(key);
        Ok(Some(Arc::new(History {
            head,
            refs,
            remotes,
            pull_configured,
            commits,
            graph,
            lanes,
            stashes,
        })))
    }

    fn stashes(&self) -> Result<Vec<StashInfo>, GitError> {
        let Ok(reference) = self.repo.find_reference("refs/stash") else {
            return Ok(Vec::new());
        };
        let mut platform = reference.log_iter();
        let Ok(Some(lines)) = platform.all() else {
            return Ok(Vec::new());
        };
        let mut stashes = Vec::new();
        for line in lines {
            let Ok(line) = line else {
                continue;
            };
            let id = line.new_oid();
            if id.is_null() {
                continue;
            }
            stashes.push(StashInfo {
                id,
                message: line.message.to_str_lossy().into_owned(),
                time: timestamp(line.signature.seconds()),
            });
        }
        stashes.reverse();
        Ok(stashes)
    }

    fn head(&self) -> Result<Head, GitError> {
        let head = self.repo.head()?;
        Ok(if head.is_unborn() {
            Head::Unborn
        } else if head.is_detached() {
            head.id()
                .map(|id| Head::Detached(id.detach()))
                .unwrap_or_default()
        } else {
            head.referent_name()
                .map(|n| Head::Branch(n.shorten().to_str_lossy().into_owned()))
                .unwrap_or_default()
        })
    }

    fn refs(&mut self) -> Result<Vec<RefInfo>, GitError> {
        let mut out = Vec::new();
        let mut locals = Vec::new();
        {
            let platform = self.repo.references()?;
            for reference in platform.all()? {
                let Ok(mut reference) = reference else {
                    continue;
                };
                let Some((kind, name)) =
                    reference
                        .name()
                        .category_and_short_name()
                        .and_then(|(category, short)| {
                            let kind = match category {
                                Category::LocalBranch => RefKind::Local,
                                Category::RemoteBranch => RefKind::Remote,
                                Category::Tag => RefKind::Tag,
                                _ => return None,
                            };
                            Some((kind, short.to_str_lossy().into_owned()))
                        })
                else {
                    continue;
                };
                if kind == RefKind::Remote && name.ends_with("/HEAD") {
                    continue;
                }
                if let Ok(id) = reference.peel_to_id() {
                    if kind == RefKind::Local {
                        locals.push((out.len(), reference.name().to_owned()));
                    }
                    out.push(RefInfo {
                        name,
                        kind,
                        target: id.detach(),
                        upstream: None,
                    });
                }
            }
        }
        for (index, name) in locals {
            let target = out[index].target;
            out[index].upstream = self.upstream(name.as_ref(), target);
        }
        Ok(out)
    }

    fn upstream(&mut self, name: &gix::refs::FullNameRef, target: Oid) -> Option<Upstream> {
        let tracking = self
            .repo
            .branch_remote_tracking_ref_name(name, gix::remote::Direction::Fetch)?
            .ok()?;
        let upstream = tracking.shorten().to_str_lossy().into_owned();
        let remote = self
            .repo
            .find_reference(tracking.as_bstr())
            .ok()
            .and_then(|mut reference| reference.peel_to_id().ok())
            .map(|id| id.detach());
        let counts = remote.and_then(|remote| self.divergence(target, remote).ok());
        Some(Upstream {
            name: upstream,
            ahead: counts.map(|counts| counts.0),
            behind: counts.map(|counts| counts.1),
        })
    }

    fn divergence(&mut self, local: Oid, upstream: Oid) -> Result<(u32, u32), GitError> {
        if let Some(counts) = self.counts.get(&(local, upstream)) {
            return Ok(*counts);
        }
        let ahead = self.count(local, upstream)?;
        let behind = self.count(upstream, local)?;
        self.counts.insert((local, upstream), (ahead, behind));
        Ok((ahead, behind))
    }

    fn count(&self, tip: Oid, hidden: Oid) -> Result<u32, GitError> {
        let mut count = 0;
        let walk = self.repo.rev_walk([tip]).with_hidden([hidden]).all()?;
        for info in walk.take(DIVERGENCE_LIMIT as usize) {
            info?;
            count += 1;
        }
        Ok(count)
    }

    fn pull_configured(&self, head: &Head) -> bool {
        let config = self.repo.config_snapshot();
        let config = config.plumbing();
        config.string_filter("pull.rebase", user_config).is_some()
            || config.string_filter("pull.ff", user_config).is_some()
            || match head {
                Head::Branch(name) => config
                    .string_filter_by("branch", name.as_str(), "rebase", user_config)
                    .is_some(),
                _ => false,
            }
    }

    fn log(&mut self, tips: Vec<Oid>) -> Result<Vec<CommitSummary>, GitError> {
        if tips.is_empty() {
            return Ok(Vec::new());
        }
        let walk = self
            .repo
            .rev_walk(tips)
            .sorting(Sorting::ByCommitTime(Default::default()))
            .all()?;
        let mut commits = Vec::new();
        for info in walk.take(LOG_LIMIT) {
            let info = info?;
            if let Some(commit) = self.cache.get(&info.id) {
                commits.push(commit.clone());
                continue;
            }
            let commit = info.object()?;
            let author = commit.author()?;
            let summary = CommitSummary {
                id: info.id,
                parents: info.parent_ids().map(|p| p.detach()).collect(),
                author: author.name.to_str_lossy().into_owned(),
                time: timestamp(author.seconds()),
                summary: commit.message()?.summary().to_str_lossy().into_owned(),
            };
            self.cache.insert(info.id, summary.clone());
            commits.push(summary);
        }
        Ok(date_order(commits))
    }

    pub fn commit(&mut self, id: Oid) -> Result<CommitDetail, GitError> {
        let commit = self.repo.find_commit(id)?;
        let tree = commit.tree()?;
        let parents: Vec<Oid> = commit.parent_ids().map(|p| p.detach()).collect();
        let parent_tree = match parents.first() {
            Some(p) => Some(self.repo.find_commit(*p)?.tree()?),
            None => None,
        };
        let hash = self.repo.object_hash();
        let changes: Vec<Resolved> = self
            .repo
            .diff_tree_to_tree(parent_tree.as_ref(), &tree, None)?
            .into_iter()
            .filter_map(|change| resolve_change(hash, change))
            .collect();
        let total = changes.len();
        let mut files = Vec::with_capacity(total);
        for resolved in changes {
            files.push(build(&self.repo, &mut self.diffs, resolved, total, false)?);
        }
        Ok(CommitDetail {
            id,
            parents,
            author: signature(commit.author()?),
            committer: signature(commit.committer()?),
            message: commit.message_raw()?.to_str_lossy().into_owned(),
            files,
        })
    }

    pub fn file(&mut self, source: FileSource, path: &str) -> Result<FileDiff, GitError> {
        let resolved = self.resolve(source, path)?;
        let version = resolved.version();
        let body = diff_body(&self.repo, &mut self.diffs, &resolved, version, FULL)?;
        let stats = self
            .diffs
            .stats(version)
            .map_or_else(|| summarize(&body).stats(), Stats::stats);
        let conflict_markers = resolved.markers();
        Ok(FileDiff {
            path: resolved.path,
            kind: resolved.kind,
            body,
            stats,
            conflict_markers,
            version,
        })
    }

    pub fn preview(
        &mut self,
        source: FileSource,
        path: &str,
        text: &str,
    ) -> Result<FileDiff, GitError> {
        let mut resolved = self.resolve(source, path)?;
        resolved.replace(Content::Data(Some(text.as_bytes().to_vec())));
        let version = resolved.version();
        let Some((old, new)) = content(&self.repo, &resolved)? else {
            return Ok(FileDiff {
                path: resolved.path,
                kind: resolved.kind,
                body: DiffBody::TooLarge,
                stats: None,
                conflict_markers: 0,
                version,
            });
        };
        let body = self.diffs.preview(&resolved.path, &old, &new, FULL);
        let stats = summarize(&body).stats();
        let conflict_markers = resolved.markers();
        Ok(FileDiff {
            path: resolved.path,
            kind: resolved.kind,
            body,
            stats,
            conflict_markers,
            version,
        })
    }

    pub fn worktree_text(&self, path: &str) -> Option<String> {
        let root = self.repo.workdir()?;
        let full = root.join(path);
        let meta = std::fs::metadata(&full).ok()?;
        if !meta.is_file() || meta.len() > MAX_FILE {
            return None;
        }
        String::from_utf8(std::fs::read(full).ok()?).ok()
    }

    pub fn hunks(
        &mut self,
        source: FileSource,
        path: &str,
        version: u64,
    ) -> Result<Arc<Vec<Hunk>>, GitError> {
        if let Some(body) = self.diffs.body(version, CONTEXT) {
            return Ok(into_hunks(body));
        }
        let resolved = self.resolve(source, path)?;
        let body = diff_body(
            &self.repo,
            &mut self.diffs,
            &resolved,
            resolved.version(),
            CONTEXT,
        )?;
        Ok(into_hunks(body))
    }

    fn resolve(&self, source: FileSource, path: &str) -> Result<Resolved, GitError> {
        let hash = self.repo.object_hash();
        let missing = || GitError::new(path.to_owned());
        Ok(match source {
            FileSource::Commit(id) => {
                let commit = self.repo.find_commit(id)?;
                let tree = commit.tree()?;
                let parent = match commit.parent_ids().next() {
                    Some(p) => Some(self.repo.find_commit(p.detach())?.tree()?),
                    None => None,
                };
                let old = entry(parent.as_ref(), path)?;
                let new = entry(Some(&tree), path)?;
                let (kind, old) = match (old, new) {
                    (Some(old), Some(new)) if old == new => (ChangeKind::Unchanged, Some(old)),
                    (Some(old), Some(_)) => (ChangeKind::Modified, Some(old)),
                    _ => {
                        let change = self
                            .repo
                            .diff_tree_to_tree(parent.as_ref(), &tree, None)?
                            .into_iter()
                            .find(|c| c.location() == path);
                        match change.and_then(|c| resolve_change(hash, c)) {
                            Some(resolved) => return Ok(resolved),
                            None => match new {
                                Some(_) => (ChangeKind::Added, None),
                                None => return Err(missing()),
                            },
                        }
                    }
                };
                Resolved::new(hash, path.to_owned(), kind, old, Content::Blob(new))
            }
            FileSource::Staged => {
                let head = self.repo.head_commit().ok().and_then(|c| c.tree().ok());
                let old = entry(head.as_ref(), path)?;
                let new = self.indexed(path)?;
                let kind = match (old, new) {
                    (None, _) => ChangeKind::Added,
                    (_, None) => ChangeKind::Deleted,
                    _ => ChangeKind::Modified,
                };
                Resolved::new(hash, path.to_owned(), kind, old, Content::Blob(new))
            }
            FileSource::Unstaged | FileSource::Untracked | FileSource::Conflicted => {
                let root = self.repo.workdir().ok_or_else(missing)?;
                let (mut pipeline, index) = self.repo.filter_pipeline(None)?;
                let exists = root.join(path).exists();
                let data = if exists {
                    worktree_file(&mut pipeline, &index, root, path)?
                } else {
                    Some(Vec::new())
                };
                let (kind, old) = match source {
                    FileSource::Untracked => (ChangeKind::Untracked, None),
                    FileSource::Conflicted => (
                        ChangeKind::Conflicted(self.conflict(path)?),
                        self.indexed(path)?,
                    ),
                    _ if !exists => (ChangeKind::Deleted, self.indexed(path)?),
                    _ => (ChangeKind::Modified, self.indexed(path)?),
                };
                Resolved::new(hash, path.to_owned(), kind, old, Content::Data(data))
            }
        })
    }

    pub fn paths(&self, commit: Option<Oid>) -> Result<Vec<String>, GitError> {
        let mut paths: Vec<String> = match commit {
            Some(id) => {
                let tree = self.repo.find_commit(id)?.tree()?;
                let mut recorder = gix::traverse::tree::Recorder::default();
                tree.traverse()
                    .breadthfirst(&mut recorder)
                    .map_err(|e| GitError::new(e.to_string()))?;
                recorder
                    .records
                    .into_iter()
                    .filter(|e| !e.mode.is_tree() && !e.mode.is_commit())
                    .map(|e| e.filepath.to_str_lossy().into_owned())
                    .collect()
            }
            None => {
                let index = self.repo.index_or_empty()?;
                index
                    .entries()
                    .iter()
                    .map(|e| e.path(&index).to_str_lossy().into_owned())
                    .collect()
            }
        };
        paths.sort_unstable();
        paths.dedup();
        Ok(paths)
    }

    fn indexed(&self, path: &str) -> Result<Option<Oid>, GitError> {
        Ok(self
            .repo
            .index_or_empty()?
            .entry_by_path(path.as_bytes().as_bstr())
            .map(|e| e.id))
    }

    fn conflict(&self, path: &str) -> Result<Conflict, GitError> {
        let index = self.repo.index_or_empty()?;
        let path = path.as_bytes().as_bstr();
        let has = |stage| index.entry_by_path_and_stage(path, stage).is_some();
        Ok(
            match (has(Stage::Base), has(Stage::Ours), has(Stage::Theirs)) {
                (true, true, true) => Conflict::BothModified,
                (false, true, true) => Conflict::BothAdded,
                (false, true, false) => Conflict::AddedByUs,
                (false, false, true) => Conflict::AddedByThem,
                (true, false, true) => Conflict::DeletedByUs,
                (true, true, false) => Conflict::DeletedByThem,
                _ => Conflict::BothDeleted,
            },
        )
    }

    fn in_progress(&self, tree: &mut WorkingTree) {
        let Some(state) = self.repo.state() else {
            return;
        };
        use gix::state::InProgress as State;
        let (operation, sequence) = match state {
            State::Merge => (Operation::Merge, false),
            State::Rebase | State::RebaseInteractive => (Operation::Rebase, false),
            State::CherryPick => (Operation::CherryPick, false),
            State::CherryPickSequence => (Operation::CherryPick, true),
            State::Revert => (Operation::Revert, false),
            State::RevertSequence => (Operation::Revert, true),
            State::Bisect => (Operation::Bisect, false),
            State::ApplyMailbox | State::ApplyMailboxRebase => (Operation::ApplyMailbox, false),
        };
        let dir = self.repo.git_dir();
        let step = match operation {
            Operation::Rebase => step(dir, "rebase-merge", "msgnum", "end")
                .or_else(|| step(dir, "rebase-apply", "next", "last")),
            Operation::ApplyMailbox => step(dir, "rebase-apply", "next", "last"),
            _ => None,
        };
        let commit = match operation {
            Operation::Merge => oid_file(&dir.join("MERGE_HEAD")),
            Operation::CherryPick => oid_file(&dir.join("CHERRY_PICK_HEAD")),
            Operation::Revert => oid_file(&dir.join("REVERT_HEAD")),
            Operation::Rebase => oid_file(&dir.join("rebase-merge").join("stopped-sha"))
                .or_else(|| oid_file(&dir.join("rebase-apply").join("original-commit"))),
            _ => None,
        };
        let target = match (operation, commit) {
            (Operation::Merge, Some(id)) => self.branch_at(id),
            _ => None,
        };
        tree.in_progress = Some(InProgress {
            operation,
            sequence,
            step,
            commit,
            target,
            message: prepared_message(dir, operation),
        });
    }

    fn branch_at(&self, id: Oid) -> Option<String> {
        let platform = self.repo.references().ok()?;
        let mut branches = platform.local_branches().ok()?;
        branches.find_map(|reference| {
            let mut reference = reference.ok()?;
            (reference.peel_to_id().ok()?.detach() == id)
                .then(|| reference.name().shorten().to_str_lossy().into_owned())
        })
    }

    fn merge_tool(&self) -> bool {
        self.repo.config_snapshot().string("merge.tool").is_some()
    }

    pub fn working(&mut self) -> Result<Arc<WorkingTree>, GitError> {
        let mut tree = WorkingTree {
            root: self.root(),
            ..WorkingTree::default()
        };
        self.in_progress(&mut tree);
        tree.merge_tool = self.merge_tool();
        let Some(root) = self.repo.workdir() else {
            return Ok(Arc::new(tree));
        };
        let (mut pipeline, index) = self.repo.filter_pipeline(None)?;
        let items = self
            .repo
            .status(gix::progress::Discard)?
            .untracked_files(UntrackedFiles::Files)
            .index_worktree_rewrites(None)
            .into_iter(Vec::<BString>::new())?;
        let hash = self.repo.object_hash();
        let mut work: Vec<(Section, Resolved)> = Vec::new();
        let mut conflicts: Vec<Resolved> = Vec::new();
        for item in items {
            match item? {
                gix::status::Item::TreeIndex(change) => {
                    let resolved = staged(hash, &change);
                    if work.iter().any(|(section, file)| {
                        *section == Section::Staged && file.path == resolved.path
                    }) {
                        continue;
                    }
                    work.push((Section::Staged, resolved));
                }
                gix::status::Item::IndexWorktree(item) => {
                    let Some(summary) = item.summary() else {
                        continue;
                    };
                    let path = item.rela_path().to_str_lossy().into_owned();
                    let indexed = match &item {
                        index_worktree::Item::Modification { entry, .. } => Some(entry.id),
                        _ => None,
                    };
                    if summary == Summary::Conflict {
                        let kind = ChangeKind::Conflicted(self.conflict(&path)?);
                        let file = Resolved::new(hash, path, kind, indexed, Content::Worktree);
                        conflicts.push(file);
                        continue;
                    }
                    let (section, kind, old, new) = match summary {
                        Summary::Added => (
                            Section::Untracked,
                            ChangeKind::Untracked,
                            None,
                            Content::Worktree,
                        ),
                        Summary::IntentToAdd => (
                            Section::Unstaged,
                            ChangeKind::Added,
                            None,
                            Content::Worktree,
                        ),
                        Summary::Removed => (
                            Section::Unstaged,
                            ChangeKind::Deleted,
                            indexed,
                            Content::Blob(None),
                        ),
                        Summary::Modified | Summary::TypeChange => (
                            Section::Unstaged,
                            ChangeKind::Modified,
                            indexed,
                            Content::Worktree,
                        ),
                        Summary::Conflict | Summary::Renamed | Summary::Copied => continue,
                    };
                    if work
                        .iter()
                        .any(|(s, file)| *s == section && file.path == path)
                    {
                        continue;
                    }
                    work.push((section, Resolved::new(hash, path, kind, old, new)));
                }
            }
        }
        let conflicted: HashSet<String> = conflicts.iter().map(|file| file.path.clone()).collect();
        work.retain(|(_, file)| !conflicted.contains(&file.path));
        let lazy = work.len() > LAZY_LIMIT;
        let total = conflicts.len() + work.len();
        for mut resolved in conflicts {
            if matches!(resolved.new, Content::Worktree) {
                let data = worktree_file(&mut pipeline, &index, root, &resolved.path)?;
                resolved.replace(Content::Data(data));
            }
            let file = build(&self.repo, &mut self.diffs, resolved, total, false)?;
            tree.conflicts.push(file);
        }
        for (section, mut resolved) in work {
            if lazy {
                if matches!(resolved.new, Content::Worktree) {
                    resolved.replace(Content::Lazy(file_stamp(root, &resolved.path)));
                }
            } else if matches!(resolved.new, Content::Worktree) {
                let data = worktree_file(&mut pipeline, &index, root, &resolved.path)?;
                resolved.replace(Content::Data(data));
            }
            let file = build(&self.repo, &mut self.diffs, resolved, total, lazy)?;
            match section {
                Section::Unstaged => tree.unstaged.push(file),
                Section::Staged => tree.staged.push(file),
                Section::Untracked => tree.untracked.push(file),
            }
        }
        for list in [
            &mut tree.conflicts,
            &mut tree.unstaged,
            &mut tree.staged,
            &mut tree.untracked,
        ] {
            list.sort_by(|a, b| a.path.cmp(&b.path));
        }
        Ok(Arc::new(tree))
    }
}

fn step(dir: &Path, name: &str, first: &str, second: &str) -> Option<(u32, u32)> {
    let dir = dir.join(name);
    let first = file_value(&dir.join(first))?.parse().ok()?;
    let second = file_value(&dir.join(second))?.parse().ok()?;
    Some((first, second))
}

fn file_value(path: &Path) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|text| text.trim().to_owned())
}

fn oid_file(path: &Path) -> Option<Oid> {
    Oid::from_hex(file_value(path)?.lines().next()?.as_bytes()).ok()
}

fn prepared_message(dir: &Path, operation: Operation) -> Option<String> {
    let raw = match operation {
        Operation::Rebase => file_value(&dir.join("rebase-merge").join("message"))
            .or_else(|| std::fs::read_to_string(dir.join("MERGE_MSG")).ok()),
        _ => std::fs::read_to_string(dir.join("MERGE_MSG")).ok(),
    }?;
    let text = raw
        .lines()
        .filter(|line| !line.starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_owned();
    (!text.is_empty()).then_some(text)
}

fn worktree_file(
    pipeline: &mut gix::filter::Pipeline<'_>,
    index: &gix::worktree::IndexPersistedOrInMemory,
    root: &Path,
    path: &str,
) -> Result<Option<Vec<u8>>, GitError> {
    let full = root.join(path);
    let meta = std::fs::metadata(&full)?;
    if meta.is_dir() {
        return Ok(Some(Vec::new()));
    }
    if meta.len() > MAX_FILE {
        return Ok(None);
    }
    let mut out = Vec::new();
    pipeline
        .convert_to_git(std::fs::File::open(&full)?, Path::new(path), index)?
        .read_to_end(&mut out)?;
    Ok(Some(out))
}

fn file_stamp(root: &Path, path: &str) -> u64 {
    let mut stamp = 0xcbf2_9ce4_8422_2325;
    let Ok(meta) = std::fs::metadata(root.join(path)) else {
        return stamp;
    };
    stamp = mix(stamp, meta.len());
    if let Ok(modified) = meta.modified()
        && let Ok(time) = modified.duration_since(std::time::UNIX_EPOCH)
    {
        stamp = mix(stamp, time.as_nanos() as u64);
    }
    stamp
}

fn build(
    repo: &gix::Repository,
    cache: &mut Cache,
    resolved: Resolved,
    total: usize,
    lazy: bool,
) -> Result<FileDiff, GitError> {
    let version = resolved.version();
    if lazy || matches!(&resolved.new, Content::Lazy(_)) {
        return Ok(FileDiff {
            path: resolved.path,
            kind: resolved.kind,
            body: DiffBody::Pending,
            stats: None,
            conflict_markers: 0,
            version,
        });
    }
    let mut computed = None;
    let stats = match cache.stats(version) {
        Some(stats) => stats,
        None if total <= FOLD_FILES => {
            let body = diff_body(repo, cache, &resolved, version, CONTEXT)?;
            let stats = summarize(&body);
            cache.remember(version, stats);
            computed = Some(body);
            stats
        }
        None => {
            let stats = match content(repo, &resolved)? {
                Some((old, new)) => measure(&resolved.path, &old, &new),
                None => Stats::TooLarge,
            };
            cache.remember(version, stats);
            stats
        }
    };
    let body = match stats {
        Stats::Binary => DiffBody::Binary,
        Stats::TooLarge => DiffBody::TooLarge,
        Stats::Text { .. } if !default_open(total, stats.stats()) => DiffBody::Pending,
        Stats::Text { .. } => match computed {
            Some(body) => body,
            None => diff_body(repo, cache, &resolved, version, CONTEXT)?,
        },
    };
    let conflict_markers = resolved.markers();
    Ok(FileDiff {
        path: resolved.path,
        kind: resolved.kind,
        body,
        stats: stats.stats(),
        conflict_markers,
        version,
    })
}

fn diff_body(
    repo: &gix::Repository,
    cache: &mut Cache,
    resolved: &Resolved,
    version: u64,
    context: u32,
) -> Result<DiffBody, GitError> {
    let Some((old, new)) = content(repo, resolved)? else {
        return Ok(DiffBody::TooLarge);
    };
    Ok(cache.diff(version, &resolved.path, &old, &new, context))
}

fn content(repo: &gix::Repository, resolved: &Resolved) -> Result<Option<Blobs>, GitError> {
    let old = match resolved.old {
        Some(id) => repo.find_blob(id)?.take_data(),
        None => Vec::new(),
    };
    let new = match &resolved.new {
        Content::Blob(id) => match id {
            Some(id) => repo.find_blob(*id)?.take_data(),
            None => Vec::new(),
        },
        Content::Data(Some(data)) => data.clone(),
        Content::Worktree | Content::Lazy(_) | Content::Data(None) => return Ok(None),
    };
    Ok(Some((old, new)))
}

fn into_hunks(body: DiffBody) -> Arc<Vec<Hunk>> {
    match body {
        DiffBody::Text(hunks) => hunks,
        _ => Arc::new(Vec::new()),
    }
}

fn entry(tree: Option<&gix::Tree<'_>>, path: &str) -> Result<Option<Oid>, GitError> {
    let Some(tree) = tree else {
        return Ok(None);
    };
    Ok(tree
        .lookup_entry_by_path(path)?
        .filter(|entry| !entry.mode().is_tree() && !entry.mode().is_commit())
        .map(|entry| entry.object_id()))
}

fn resolve_change(hash: gix::hash::Kind, change: ChangeDetached) -> Option<Resolved> {
    let (path, kind, old, new, mode) = match change {
        ChangeDetached::Addition {
            location,
            entry_mode,
            id,
            ..
        } => (location, ChangeKind::Added, None, Some(id), entry_mode),
        ChangeDetached::Deletion {
            location,
            entry_mode,
            id,
            ..
        } => (location, ChangeKind::Deleted, Some(id), None, entry_mode),
        ChangeDetached::Modification {
            location,
            previous_id,
            entry_mode,
            id,
            ..
        } => (
            location,
            ChangeKind::Modified,
            Some(previous_id),
            Some(id),
            entry_mode,
        ),
        ChangeDetached::Rewrite {
            source_location,
            source_id,
            entry_mode,
            id,
            location,
            copy,
            ..
        } => {
            let source = source_location.to_str_lossy().into_owned();
            let kind = if copy {
                ChangeKind::Copied(source)
            } else {
                ChangeKind::Renamed(source)
            };
            (location, kind, Some(source_id), Some(id), entry_mode)
        }
    };
    if mode.is_tree() || mode.is_commit() {
        return None;
    }
    let path = path.to_str_lossy().into_owned();
    Some(Resolved::new(hash, path, kind, old, Content::Blob(new)))
}

fn staged(hash: gix::hash::Kind, change: &ChangeRef<'_, '_>) -> Resolved {
    let (kind, old, new) = match change {
        ChangeRef::Addition { id, .. } => (ChangeKind::Added, None, Some(id.clone().into_owned())),
        ChangeRef::Deletion { id, .. } => {
            (ChangeKind::Deleted, Some(id.clone().into_owned()), None)
        }
        ChangeRef::Modification {
            previous_id, id, ..
        } => (
            ChangeKind::Modified,
            Some(previous_id.clone().into_owned()),
            Some(id.clone().into_owned()),
        ),
        ChangeRef::Rewrite {
            source_location,
            source_id,
            id,
            copy,
            ..
        } => {
            let source = source_location.to_str_lossy().into_owned();
            (
                if *copy {
                    ChangeKind::Copied(source)
                } else {
                    ChangeKind::Renamed(source)
                },
                Some(source_id.clone().into_owned()),
                Some(id.clone().into_owned()),
            )
        }
    };
    Resolved::new(
        hash,
        change.location().to_str_lossy().into_owned(),
        kind,
        old,
        Content::Blob(new),
    )
}

fn kind_code(kind: &ChangeKind) -> u64 {
    match kind {
        ChangeKind::Added => 1,
        ChangeKind::Deleted => 2,
        ChangeKind::Modified => 3,
        ChangeKind::Renamed(_) => 4,
        ChangeKind::Copied(_) => 5,
        ChangeKind::Conflicted(conflict) => 6 + conflict_code(conflict),
        ChangeKind::Untracked => 13,
        ChangeKind::Unchanged => 14,
    }
}

fn conflict_code(conflict: &Conflict) -> u64 {
    match conflict {
        Conflict::BothModified => 0,
        Conflict::BothAdded => 1,
        Conflict::AddedByUs => 2,
        Conflict::AddedByThem => 3,
        Conflict::DeletedByUs => 4,
        Conflict::DeletedByThem => 5,
        Conflict::BothDeleted => 6,
    }
}

fn mix(hash: u64, value: u64) -> u64 {
    (hash ^ value).wrapping_mul(0x0100_0000_01b3)
}

fn signature(sig: gix::actor::SignatureRef<'_>) -> Signature {
    Signature {
        name: sig.name.to_str_lossy().into_owned(),
        email: sig.email.to_str_lossy().into_owned(),
        time: timestamp(sig.seconds()),
    }
}

fn timestamp(seconds: i64) -> jiff::Timestamp {
    jiff::Timestamp::from_second(seconds).unwrap_or_default()
}

fn open_error(error: gix::Error) -> GitError {
    if matches!(
        error.downcast_any_ref::<gix::discover::upwards::Error>(),
        Some(
            gix::discover::upwards::Error::NoGitRepository { .. }
                | gix::discover::upwards::Error::NoGitRepositoryWithinCeiling { .. }
                | gix::discover::upwards::Error::NoGitRepositoryWithinFs { .. }
        )
    ) {
        return GitError::missing_repository();
    }
    GitError::from(error)
}

fn user_config(meta: &gix::config::file::Metadata) -> bool {
    !matches!(
        meta.source,
        gix::config::Source::System | gix::config::Source::GitInstallation
    )
}

fn date_order(commits: Vec<CommitSummary>) -> Vec<CommitSummary> {
    let index: HashMap<Oid, usize> = commits.iter().enumerate().map(|(i, c)| (c.id, i)).collect();
    let mut children = vec![0u32; commits.len()];
    for c in &commits {
        for p in &c.parents {
            if let Some(&j) = index.get(p) {
                children[j] += 1;
            }
        }
    }
    let mut ready: BinaryHeap<Reverse<usize>> = (0..commits.len())
        .filter(|&i| children[i] == 0)
        .map(Reverse)
        .collect();
    let mut order = Vec::with_capacity(commits.len());
    while let Some(Reverse(i)) = ready.pop() {
        order.push(i);
        for p in &commits[i].parents {
            if let Some(&j) = index.get(p) {
                children[j] -= 1;
                if children[j] == 0 {
                    ready.push(Reverse(j));
                }
            }
        }
    }
    let mut slots: Vec<Option<CommitSummary>> = commits.into_iter().map(Some).collect();
    order.into_iter().filter_map(|i| slots[i].take()).collect()
}

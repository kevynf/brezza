use std::ops::Range;
use std::sync::Arc;

pub type Oid = gix::ObjectId;

pub const FOLD_FILES: usize = 8;
pub const LAZY_LIMIT: usize = 50;
pub const MAX_FILE: u64 = 2 << 20;
const FOLD_LINES: u32 = 300;

pub fn default_open(total: usize, stats: Option<(u32, u32)>) -> bool {
    total <= FOLD_FILES && stats.is_none_or(|(added, removed)| added + removed <= FOLD_LINES)
}

#[derive(Clone, Debug)]
pub struct Signature {
    pub name: String,
    pub email: String,
    pub time: jiff::Timestamp,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefKind {
    Local,
    Remote,
    Tag,
}

#[derive(Clone, Debug)]
pub struct Upstream {
    pub name: String,
    pub ahead: Option<u32>,
    pub behind: Option<u32>,
}

#[derive(Clone, Debug)]
pub struct RefInfo {
    pub name: String,
    pub kind: RefKind,
    pub target: Oid,
    pub upstream: Option<Upstream>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Head {
    Branch(String),
    Detached(Oid),
    #[default]
    Unborn,
}

#[derive(Clone, Debug)]
pub struct CommitSummary {
    pub id: Oid,
    pub parents: Vec<Oid>,
    pub author: String,
    pub time: jiff::Timestamp,
    pub summary: String,
}

#[derive(Clone, Debug)]
pub struct CommitDetail {
    pub id: Oid,
    pub parents: Vec<Oid>,
    pub author: Signature,
    pub committer: Signature,
    pub message: String,
    pub files: Vec<FileDiff>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Conflict {
    BothModified,
    BothAdded,
    AddedByUs,
    AddedByThem,
    DeletedByUs,
    DeletedByThem,
    BothDeleted,
}

impl Conflict {
    pub fn ours(self) -> bool {
        !matches!(self, Self::DeletedByUs | Self::BothDeleted)
    }

    pub fn theirs(self) -> bool {
        !matches!(self, Self::DeletedByThem | Self::BothDeleted)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChangeKind {
    Added,
    Deleted,
    Modified,
    Renamed(String),
    Copied(String),
    Conflicted(Conflict),
    Untracked,
    Unchanged,
}

#[derive(Clone, Debug)]
pub struct FileDiff {
    pub path: String,
    pub kind: ChangeKind,
    pub body: DiffBody,
    pub stats: Option<(u32, u32)>,
    pub conflict_markers: u32,
    pub version: u64,
}

impl FileDiff {
    pub fn change_starts(&self) -> Vec<usize> {
        let DiffBody::Text(hunks) = &self.body else {
            return Vec::new();
        };
        let Some(lines) = hunks.first().map(|h| &h.lines) else {
            return Vec::new();
        };
        (0..lines.len())
            .filter(|&i| {
                lines[i].kind != LineKind::Context
                    && (i == 0 || lines[i - 1].kind == LineKind::Context)
            })
            .collect()
    }

    pub fn stats(&self) -> Option<(u32, u32)> {
        self.stats
    }
}

#[derive(Clone, Debug)]
pub enum DiffBody {
    Text(Arc<Vec<Hunk>>),
    Pending,
    Binary,
    TooLarge,
}

#[derive(Clone, Debug)]
pub struct Hunk {
    pub old_start: u32,
    pub old_len: u32,
    pub new_start: u32,
    pub new_len: u32,
    pub lines: Vec<DiffLine>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineKind {
    Context,
    Added,
    Removed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Syntax {
    Comment,
    Function,
    Keyword,
    Literal,
    String,
    Punctuation,
}

pub type SyntaxSpans = Vec<(Range<usize>, Syntax)>;

#[derive(Clone, Debug)]
pub struct DiffLine {
    pub kind: LineKind,
    pub old_no: Option<u32>,
    pub new_no: Option<u32>,
    pub text: String,
    pub emphasis: Vec<Range<usize>>,
    pub syntax: SyntaxSpans,
    pub no_eol: bool,
}

#[derive(Clone, Debug, Default)]
pub struct WorkingTree {
    pub root: std::path::PathBuf,
    pub conflicts: Vec<FileDiff>,
    pub unstaged: Vec<FileDiff>,
    pub staged: Vec<FileDiff>,
    pub untracked: Vec<FileDiff>,
    pub in_progress: Option<InProgress>,
    pub merge_tool: bool,
}

impl WorkingTree {
    pub fn is_clean(&self) -> bool {
        self.conflicts.is_empty()
            && self.unstaged.is_empty()
            && self.staged.is_empty()
            && self.untracked.is_empty()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operation {
    Merge,
    Rebase,
    CherryPick,
    Revert,
    Bisect,
    ApplyMailbox,
}

#[derive(Clone, Debug)]
pub struct InProgress {
    pub operation: Operation,
    pub sequence: bool,
    pub step: Option<(u32, u32)>,
    pub commit: Option<Oid>,
    pub target: Option<String>,
    pub message: Option<String>,
}

pub const UNCOMMITTED: u16 = u16::MAX;

pub const DIVERGENCE_LIMIT: u32 = 1000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GraphEdge {
    pub from: u16,
    pub to: u16,
    pub color: u16,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GraphRow {
    pub lane: u16,
    pub color: u16,
    pub head: bool,
    pub edges: Vec<GraphEdge>,
}

#[derive(Clone, Debug)]
pub struct StashInfo {
    pub id: Oid,
    pub message: String,
    pub time: jiff::Timestamp,
}

impl StashInfo {
    pub fn summary(&self) -> &str {
        let message = self.message.as_str();
        if let Some(rest) = message.strip_prefix("WIP on ") {
            return rest
                .split_once(": ")
                .map_or(message, |(_, summary)| strip_hash(summary));
        }
        if message.starts_with("On ") {
            return message
                .split_once(": ")
                .map_or(message, |(_, summary)| summary);
        }
        message
    }
}

fn strip_hash(summary: &str) -> &str {
    summary
        .split_once(' ')
        .filter(|(hash, _)| hash.len() >= 7 && hash.bytes().all(|b| b.is_ascii_hexdigit()))
        .map_or(summary, |(_, rest)| rest)
}

#[derive(Clone, Debug, Default)]
pub struct History {
    pub head: Head,
    pub refs: Vec<RefInfo>,
    pub remotes: Vec<String>,
    pub pull_configured: bool,
    pub commits: Vec<CommitSummary>,
    pub graph: Vec<GraphRow>,
    pub lanes: u16,
    pub stashes: Vec<StashInfo>,
}

impl History {
    pub fn local(&self, name: &str) -> Option<&RefInfo> {
        self.refs
            .iter()
            .find(|r| r.kind == RefKind::Local && r.name == name)
    }

    pub fn remote_branch<'a>(&'a self, name: &'a str) -> Option<(&'a str, &'a str)> {
        self.remotes
            .iter()
            .filter_map(|remote| {
                let branch = name.strip_prefix(remote.as_str())?.strip_prefix('/')?;
                Some((remote.as_str(), branch))
            })
            .max_by_key(|(remote, _)| remote.len())
    }
}

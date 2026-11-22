use std::sync::Arc;

use crate::model::{CommitDetail, FileDiff, History, Hunk, Oid, Operation, WorkingTree};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FileSource {
    Commit(Oid),
    Conflicted,
    Staged,
    Unstaged,
    Untracked,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PullMode {
    Configured,
    FastForward,
    Merge,
    Rebase,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResetMode {
    Soft,
    Mixed,
    Hard,
}

#[derive(Clone, Debug)]
pub enum Request {
    Refresh,
    Commit(Oid),
    File {
        source: FileSource,
        path: String,
    },
    Preview {
        source: FileSource,
        path: String,
        text: String,
        generation: u64,
    },
    Hunks {
        source: FileSource,
        path: String,
        version: u64,
    },
    Paths(Option<Oid>),
    Run(Command),
}

#[derive(Clone, Debug)]
pub enum Response {
    History(Arc<History>),
    Working(Arc<WorkingTree>),
    Commit(Box<CommitDetail>),
    File {
        source: FileSource,
        diff: Box<FileDiff>,
        text: Option<String>,
    },
    Preview {
        source: FileSource,
        generation: u64,
        diff: Box<FileDiff>,
    },
    Hunks {
        version: u64,
        hunks: Arc<Vec<Hunk>>,
    },
    Paths {
        commit: Option<Oid>,
        paths: Vec<String>,
    },
    Done,
}

#[derive(Clone, Debug)]
pub enum Command {
    Stage(Vec<String>),
    Unstage(Vec<String>),
    Discard(Vec<String>),
    DeleteUntracked(Vec<String>),
    ApplyPatch {
        patch: String,
        reverse: bool,
    },
    Commit {
        message: String,
        amend: bool,
    },
    Checkout(String),
    CheckoutDetached(Oid),
    TrackBranch {
        branch: String,
        remote: String,
    },
    CreateBranch {
        name: String,
        at: Oid,
    },
    RenameBranch {
        from: String,
        to: String,
    },
    DeleteBranch {
        name: String,
        force: bool,
    },
    SetUpstream {
        remote: String,
        branch: String,
    },
    UnsetUpstream(String),
    CreateTag {
        name: String,
        at: Oid,
    },
    DeleteTag(String),
    CherryPick(Oid),
    Revert(Oid),
    Reset {
        to: Oid,
        mode: ResetMode,
    },
    Merge {
        target: String,
        no_ff: bool,
        autostash: bool,
    },
    Rebase {
        target: String,
        autostash: bool,
    },
    Continue {
        operation: Operation,
        sequence: bool,
        message: String,
    },
    Skip(Operation),
    Abort(Operation),
    Resolve {
        ours: Vec<String>,
        theirs: Vec<String>,
        remove: Vec<String>,
        stage: Vec<String>,
    },
    MergeTool(String),
    Fetch,
    Pull(PullMode),
    Push {
        force: bool,
        set_upstream: Option<String>,
    },
    PushTag {
        remote: Option<String>,
        tag: String,
    },
    DeleteRemoteBranch {
        remote: String,
        branch: String,
    },
    StashPush {
        message: Option<String>,
        include_untracked: bool,
    },
    StashPop,
    StashApply {
        index: usize,
        id: Oid,
    },
    StashPopAt {
        index: usize,
        id: Oid,
    },
    StashDrop {
        index: usize,
        id: Oid,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Recovery {
    ForcePush { set_upstream: Option<String> },
    ForceDeleteBranch(String),
    PullMerge,
    PullRebase,
}

#[derive(Clone, Debug, thiserror::Error)]
#[error("{message}")]
pub struct GitError {
    pub message: String,
    pub recovery: Vec<Recovery>,
    pub missing_repository: bool,
}

impl GitError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            recovery: Vec::new(),
            missing_repository: false,
        }
    }

    pub fn missing_repository() -> Self {
        Self {
            message: crate::fl!("not-a-repository"),
            recovery: Vec::new(),
            missing_repository: true,
        }
    }
}

impl Command {
    pub fn with_autostash(self, autostash: bool) -> Self {
        match self {
            Command::Merge { target, no_ff, .. } => Command::Merge {
                target,
                no_ff,
                autostash,
            },
            Command::Rebase { target, .. } => Command::Rebase { target, autostash },
            other => other,
        }
    }
}

impl From<gix::Error> for GitError {
    fn from(error: gix::Error) -> Self {
        Self::new(error.to_string())
    }
}

impl From<std::io::Error> for GitError {
    fn from(error: std::io::Error) -> Self {
        Self::new(error.to_string())
    }
}

use std::io::Write;
use std::path::Path;
use std::process::{Command as Process, Stdio};

use super::protocol::{Command, GitError, PullMode, Recovery, ResetMode};
use crate::model::{Oid, Operation};

pub fn run(root: &Path, command: &Command) -> Result<(), GitError> {
    let (args, input): (Vec<String>, Option<&str>) = match command {
        Command::Stage(p) => (paths(&["add", "-A"], p), None),
        Command::Unstage(p) => (paths(&["reset", "-q"], p), None),
        Command::Discard(p) => (paths(&["checkout", "-q"], p), None),
        Command::DeleteUntracked(p) => {
            for path in p {
                std::fs::remove_file(root.join(path))?;
            }
            return Ok(());
        }
        Command::ApplyPatch { patch, reverse } => {
            let mut args = vec!["apply", "--cached", "--whitespace=nowarn"];
            if *reverse {
                args.push("--reverse");
            }
            (strings(&args), Some(patch))
        }
        Command::Commit { message, amend } => {
            let mut args = vec!["commit", "-q", "-F", "-"];
            if *amend {
                args.push("--amend");
            }
            (strings(&args), Some(message))
        }
        Command::Checkout(name) => (strings(&["checkout", "-q", name]), None),
        Command::CheckoutDetached(id) => (
            strings(&["checkout", "-q", "--detach", &id.to_string()]),
            None,
        ),
        Command::TrackBranch { branch, remote } => {
            let tracking = format!("{remote}/{branch}");
            (
                strings(&["checkout", "-q", "-b", branch, "--track", &tracking]),
                None,
            )
        }
        Command::CreateBranch { name, at } => (
            strings(&["checkout", "-q", "-b", name, &at.to_string()]),
            None,
        ),
        Command::RenameBranch { from, to } => (strings(&["branch", "-m", from, to]), None),
        Command::DeleteBranch { name, force } => (
            strings(&["branch", if *force { "-D" } else { "-d" }, name]),
            None,
        ),
        Command::SetUpstream { remote, branch } => (
            vec![
                "branch".to_owned(),
                format!("--set-upstream-to={remote}/{branch}"),
            ],
            None,
        ),
        Command::UnsetUpstream(name) => (strings(&["branch", "--unset-upstream", name]), None),
        Command::CreateTag { name, at } => (strings(&["tag", name, &at.to_string()]), None),
        Command::DeleteTag(name) => (strings(&["tag", "-d", name]), None),
        Command::CherryPick(id) => (strings(&["cherry-pick", &id.to_string()]), None),
        Command::Revert(id) => (strings(&["revert", "--no-edit", &id.to_string()]), None),
        Command::Reset { to, mode } => (
            strings(&[
                "reset",
                "-q",
                match mode {
                    ResetMode::Soft => "--soft",
                    ResetMode::Mixed => "--mixed",
                    ResetMode::Hard => "--hard",
                },
                &to.to_string(),
            ]),
            None,
        ),
        Command::Merge {
            target,
            no_ff,
            autostash,
        } => {
            let mut args = vec!["merge".to_owned()];
            if *no_ff {
                args.push("--no-ff".to_owned());
            }
            if *autostash {
                args.push("--autostash".to_owned());
            }
            args.push(target.clone());
            (args, None)
        }
        Command::Rebase { target, autostash } => {
            let mut args = vec!["rebase".to_owned()];
            if *autostash {
                args.push("--autostash".to_owned());
            }
            args.push(target.clone());
            (args, None)
        }
        Command::Continue {
            operation,
            sequence,
            message,
        } => match operation {
            Operation::Rebase => (strings(&["rebase", "--continue"]), None),
            Operation::ApplyMailbox => (strings(&["am", "--continue"]), None),
            Operation::CherryPick if *sequence => (strings(&["cherry-pick", "--continue"]), None),
            Operation::Revert if *sequence => (strings(&["revert", "--continue"]), None),
            _ => (
                strings(&["commit", "-q", "-F", "-"]),
                Some(message.as_str()),
            ),
        },
        Command::Skip(operation) => match operation {
            Operation::Rebase => (strings(&["rebase", "--skip"]), None),
            Operation::CherryPick => (strings(&["cherry-pick", "--skip"]), None),
            Operation::Revert => (strings(&["revert", "--skip"]), None),
            Operation::ApplyMailbox => (strings(&["am", "--skip"]), None),
            Operation::Merge | Operation::Bisect => return Ok(()),
        },
        Command::Abort(operation) => (
            match operation {
                Operation::Merge => strings(&["merge", "--abort"]),
                Operation::Rebase => strings(&["rebase", "--abort"]),
                Operation::CherryPick => strings(&["cherry-pick", "--abort"]),
                Operation::Revert => strings(&["revert", "--abort"]),
                Operation::ApplyMailbox => strings(&["am", "--abort"]),
                Operation::Bisect => strings(&["bisect", "reset"]),
            },
            None,
        ),
        Command::Resolve {
            ours,
            theirs,
            remove,
            stage,
        } => return resolve(root, command, ours, theirs, remove, stage),
        Command::MergeTool(path) => (
            paths(&["mergetool", "--no-prompt"], std::slice::from_ref(path)),
            None,
        ),
        Command::Fetch => (strings(&["fetch", "--all", "--prune"]), None),
        Command::Pull(mode) => (
            match mode {
                PullMode::Configured => strings(&["pull"]),
                PullMode::FastForward => strings(&["pull", "--ff-only"]),
                PullMode::Merge => strings(&["pull", "--no-rebase"]),
                PullMode::Rebase => strings(&["pull", "--rebase"]),
            },
            None,
        ),
        Command::Push {
            force,
            set_upstream,
        } => {
            let mut args = vec!["push".to_owned()];
            if *force {
                args.push("--force-with-lease".to_owned());
            }
            if let Some(remote) = set_upstream {
                args.extend([
                    "--set-upstream".to_owned(),
                    remote.clone(),
                    "HEAD".to_owned(),
                ]);
            }
            (args, None)
        }
        Command::PushTag { remote, tag } => {
            let mut args = vec!["push".to_owned()];
            args.extend(remote.iter().cloned());
            args.push(format!("refs/tags/{tag}"));
            (args, None)
        }
        Command::DeleteRemoteBranch { remote, branch } => {
            (strings(&["push", remote, "--delete", branch]), None)
        }
        Command::StashPush {
            message,
            include_untracked,
        } => {
            let mut args = vec!["stash".to_owned(), "push".to_owned()];
            if *include_untracked {
                args.push("--include-untracked".to_owned());
            }
            if let Some(message) = message {
                args.extend(["-m".to_owned(), message.clone()]);
            }
            (args, None)
        }
        Command::StashPop => (strings(&["stash", "pop"]), None),
        Command::StashApply { index, id } => {
            verify_stash(root, *index, *id)?;
            (
                strings(&["stash", "apply", "--index", &stash(*index)]),
                None,
            )
        }
        Command::StashPopAt { index, id } => {
            verify_stash(root, *index, *id)?;
            (strings(&["stash", "pop", "--index", &stash(*index)]), None)
        }
        Command::StashDrop { index, id } => {
            verify_stash(root, *index, *id)?;
            (strings(&["stash", "drop", &stash(*index)]), None)
        }
    };
    git(root, command, &args, input)
}

fn resolve(
    root: &Path,
    command: &Command,
    ours: &[String],
    theirs: &[String],
    remove: &[String],
    stage: &[String],
) -> Result<(), GitError> {
    if !remove.is_empty() {
        git(root, command, &paths(&["rm", "-q", "-f"], remove), None)?;
    }
    let mut checked = Vec::new();
    for (side, files) in [("--ours", ours), ("--theirs", theirs)] {
        if files.is_empty() {
            continue;
        }
        checked.extend(files.iter().cloned());
        git(root, command, &paths(&["checkout", side], files), None)?;
    }
    if !checked.is_empty() {
        git(root, command, &paths(&["add", "-A"], &checked), None)?;
    }
    if !stage.is_empty() {
        git(root, command, &paths(&["add", "-A"], stage), None)?;
    }
    Ok(())
}

fn paths(args: &[&str], paths: &[String]) -> Vec<String> {
    args.iter()
        .map(|s| s.to_string())
        .chain(["--".to_owned()])
        .chain(paths.iter().cloned())
        .collect()
}

fn stash(index: usize) -> String {
    format!("stash@{{{index}}}")
}

fn verify_stash(root: &Path, index: usize, id: Oid) -> Result<(), GitError> {
    let Ok(current) = stdout(root, &strings(&["rev-parse", "--verify", &stash(index)])) else {
        return Err(GitError::new(crate::fl!("stash-stale")));
    };
    if current.trim() == id {
        Ok(())
    } else {
        Err(GitError::new(crate::fl!("stash-stale")))
    }
}

fn strings(args: &[&str]) -> Vec<String> {
    args.iter().map(|s| s.to_string()).collect()
}

fn git(
    root: &Path,
    command: &Command,
    args: &[String],
    input: Option<&str>,
) -> Result<(), GitError> {
    let output = execute(root, args, input)?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    if stdout.contains("CONFLICT") || stderr.contains("CONFLICT") {
        return Ok(());
    }
    let mut error = failed(&output);
    error.recovery = recovery(command, &stderr.to_lowercase());
    Err(error)
}

fn stdout(root: &Path, args: &[String]) -> Result<String, GitError> {
    let output = execute(root, args, None)?;
    if !output.status.success() {
        return Err(failed(&output));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[expect(clippy::disallowed_methods)]
fn execute(
    root: &Path,
    args: &[String],
    input: Option<&str>,
) -> Result<std::process::Output, GitError> {
    let mut process = Process::new("git");
    process
        .current_dir(root)
        .args(args)
        .env("LC_ALL", "C")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_EDITOR", "true")
        .env("GIT_MERGE_AUTOEDIT", "no")
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    std::os::windows::process::CommandExt::creation_flags(&mut process, 0x0800_0000);
    let mut child = process.spawn()?;
    if let (Some(input), Some(mut stdin)) = (input, child.stdin.take()) {
        stdin.write_all(input.as_bytes())?;
    }
    Ok(child.wait_with_output()?)
}

fn failed(output: &std::process::Output) -> GitError {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let message = if stderr.trim().is_empty() {
        stdout
    } else {
        stderr
    };
    GitError::new(message.trim())
}

pub fn clone(url: &str, dir: &Path) -> Result<(), GitError> {
    let parent = dir
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let name = dir.file_name().map_or_else(
        || dir.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    let args = strings(&["clone", "--", url, name.as_str()]);
    checked(execute(parent, &args, None)?)
}

pub fn init(dir: &Path) -> Result<(), GitError> {
    checked(execute(dir, &strings(&["init"]), None)?)
}

fn checked(output: std::process::Output) -> Result<(), GitError> {
    if output.status.success() {
        Ok(())
    } else {
        Err(failed(&output))
    }
}

fn recovery(command: &Command, stderr: &str) -> Vec<Recovery> {
    match command {
        Command::Push {
            force: false,
            set_upstream,
        } if stderr.contains("non-fast-forward") || stderr.contains("fetch first") => {
            vec![Recovery::ForcePush {
                set_upstream: set_upstream.clone(),
            }]
        }
        Command::DeleteBranch { name, force: false } if stderr.contains("not fully merged") => {
            vec![Recovery::ForceDeleteBranch(name.clone())]
        }
        Command::Pull(PullMode::FastForward) if stderr.contains("not possible to fast-forward") => {
            vec![Recovery::PullMerge, Recovery::PullRebase]
        }
        _ => Vec::new(),
    }
}

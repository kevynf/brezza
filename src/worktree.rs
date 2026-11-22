use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::git::protocol::GitError;

static TEMP_ID: AtomicU64 = AtomicU64::new(0);

pub fn read_file(path: &Path) -> std::io::Result<Option<Vec<u8>>> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

pub fn worktree_path(root: &Path, path: &str) -> Result<PathBuf, GitError> {
    let relative = Path::new(path);
    let valid = !relative.as_os_str().is_empty()
        && !relative.is_absolute()
        && relative
            .components()
            .all(|component| matches!(component, Component::Normal(_)));
    if !valid {
        return Err(GitError::new(format!("{path}: invalid path")));
    }
    let root = root
        .canonicalize()
        .map_err(|error| GitError::new(format!("{path}: {error}")))?;
    let full = root.join(relative);
    let parent = full
        .parent()
        .ok_or_else(|| GitError::new(format!("{path}: invalid path")))?
        .canonicalize()
        .map_err(|error| GitError::new(format!("{path}: {error}")))?;
    if !parent.starts_with(&root) {
        return Err(GitError::new(format!("{path}: outside the worktree")));
    }
    if let Ok(metadata) = std::fs::symlink_metadata(&full)
        && metadata.file_type().is_symlink()
    {
        let target = full
            .canonicalize()
            .map_err(|error| GitError::new(format!("{path}: {error}")))?;
        if !target.starts_with(&root) {
            return Err(GitError::new(format!("{path}: outside the worktree")));
        }
    }
    Ok(full)
}

pub fn write_file(full: &Path, data: &[u8]) -> std::io::Result<()> {
    let symlink = std::fs::symlink_metadata(full).is_ok_and(|meta| meta.file_type().is_symlink());
    if symlink {
        return write_direct(full, data);
    }
    let permissions = std::fs::metadata(full).ok().map(|meta| meta.permissions());
    let temp = temp_path(full)?;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)?;
    let written = {
        use std::io::Write;
        file.write_all(data).and_then(|()| file.sync_all())
    };
    drop(file);
    let result = written
        .and_then(|()| {
            if let Some(permissions) = permissions {
                std::fs::set_permissions(&temp, permissions)?;
            }
            Ok(())
        })
        .and_then(|()| std::fs::rename(&temp, full));
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}

fn temp_path(full: &Path) -> std::io::Result<PathBuf> {
    let parent = full
        .parent()
        .ok_or_else(|| std::io::Error::other("missing parent directory"))?;
    let name = full
        .file_name()
        .map(|name| name.to_string_lossy())
        .unwrap_or_default();
    let id = TEMP_ID.fetch_add(1, Ordering::Relaxed);
    Ok(parent.join(format!(".{name}.brezza-{}-{id}.tmp", std::process::id())))
}

fn write_direct(path: &Path, data: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)?;
    file.write_all(data)?;
    file.sync_all()
}

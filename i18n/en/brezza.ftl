commits = Commits
files = Files
changed-files = Changed files
summary = Summary
open-full-file = Open the full file
previous-change = Previous change
next-change = Next change
diff-unified = Unified
diff-split = Split
save = Save (Ctrl+S)
unsaved-changes = { $count ->
    [one] This file has unsaved changes.
   *[other] { $count } files have unsaved changes.
}
unsaved-changes-title = Unsaved changes
save-all-exit = Save all and exit
discard-all-exit = Discard all and exit
expand-all = Expand all
collapse-all = Collapse all
file-changed-on-disk = File changed on disk
confirm-reload = Reload this file and discard unsaved changes?
reload = Reload
overwrite = Overwrite
editing-working-tree = Externally staged; still editing the working-tree version
preview-error = Diff preview failed: { $message }
branches = Branches
remotes = Remotes
tags = Tags
no-changes = No changes
commit-changes = Commit changes
checkout = Checkout
checkout-commit = Checkout this commit
create-branch = Create branch…
create-tag = Create tag…
rename-branch = Rename branch…
rename-branch-title = Rename branch
rename = Rename
unset-upstream = Unset upstream
set-upstream = Set as upstream of current branch
cherry-pick = Cherry-pick
revert = Revert
reset-to-here = Reset current branch to here
copy-hash = Copy hash
copy-name = Copy name
delete-branch = Delete branch
delete-tag = Delete tag
commit-hash = Commit
author = Author
committer = Committer
date = Date
parents = Parents
unstaged = Unstaged files
staged = Staged files
untracked = Untracked files
commit-message = Commit message
amend = Amend previous commit
commit = Commit
stage = Stage
stage-all = Stage all
unstage = Unstage
unstage-all = Unstage all
discard = Discard
discard-all = Discard all
delete = Delete
delete-all = Delete all
stage-hunk = Stage hunk
unstage-hunk = Unstage hunk
confirm-discard = { $count ->
    [one] Discard changes to this file? This cannot be undone.
   *[other] Discard changes to { $count } files? This cannot be undone.
}
confirm-delete = { $count ->
    [one] Delete this untracked file? This cannot be undone.
   *[other] Delete { $count } untracked files? This cannot be undone.
}
binary-file = Binary file, no text diff to show
file-too-large = File too large, diff not shown
open-repository = Open repository…
clone-repository = Clone repository…
clone-title = Clone repository
clone-url = Repository URL
clone-parent = Parent folder
clone-name = Repository name
choose-folder = Choose folder…
clone = Clone
clone-url-required = Enter a repository URL.
clone-target-exists = The target folder already exists and is not empty.
cloning = Cloning { $name }…
new-repository = New repository…
initializing = Initializing { $name }…
init-here = Initialize repository here
recent-repositories = Recent repositories
clear-recent = Clear recent repositories
toggle-sidebar = Toggle sidebar
refresh = Refresh
fetch = Fetch
pull = Pull
push = Push
stash = Stash
stash-push = Stash changes
stash-push-custom = Stash changes…
stash-pop = Pop stash
stashes = Stashes
stash-message = Message (optional)
stash-untracked = Include untracked files
stash-apply = Apply stash
stash-drop = Drop stash
confirm-stash-drop = Drop this stash? This cannot be undone.
stash-stale = The stash list changed. Review the stashes and try again.
not-a-repository = This folder is not a Git repository
language = Language
theme = Theme
theme-system = Follow system
theme-light = Light
theme-dark = Dark
settings = Settings
search-commits = Search commits
type-command = Type a command
branch-name = Branch name
tag-name = Tag name
create = Create
cancel = Cancel
dismiss = Dismiss

weekday = { $wd ->
    [1] Mon
    [2] Tue
    [3] Wed
    [4] Thu
    [5] Fri
    [6] Sat
   *[7] Sun
}
month = { $month ->
    [1] Jan
    [2] Feb
    [3] Mar
    [4] Apr
    [5] May
    [6] Jun
    [7] Jul
    [8] Aug
    [9] Sep
    [10] Oct
    [11] Nov
   *[12] Dec
}
date-this-year = { weekday }, { month } { $day }, { $time }
date-older = { month } { $day }, { $year }
date-full = { weekday }, { month } { $day }, { $year }, { $time }
tree-view = Tree view
flat-list = Flat list
new-tab = New tab
wrap-lines = Wrap lines
scroll-lines = Scroll horizontally
show-changes = Changes
show-all = All
all-files = All files
uncommitted-changes = Uncommitted changes
force-push = Force push
force-delete-branch = Force delete branch
pull-merge = Merge instead
pull-rebase = Rebase instead
upstream-gone = Upstream gone
push-tag = Push tag
delete-remote-branch = Delete remote branch
choose-remote = Choose remote
merge-into-current = Merge into current branch
merge-no-fast-forward = Merge into current branch (no fast-forward)
rebase-onto = Rebase current branch onto here
autostash-hint = The working tree has uncommitted changes.
autostash = Stash changes automatically
continue = Continue
confirm-delete-remote = Delete branch { $branch } from { $remote }? Everyone using the remote will lose it.
confirm-force-push = Force pushing overwrites the remote branch and discards commits that are not available locally. Continue?
confirm-force-delete = Branch { $name } is not fully merged. Delete it anyway? Its commits may become unreachable.
conflicts = Conflicts
conflict-both-modified = Both modified
conflict-both-added = Both added
conflict-added-by-us = Added by us
conflict-added-by-them = Added by them
conflict-deleted-by-us = Deleted by us
conflict-deleted-by-them = Deleted by them
conflict-both-deleted = Both deleted
conflict-markers = { $count ->
    [one] 1 conflict marker
   *[other] { $count } conflict markers
}
conflict-actions = Conflict actions
use-ours = Use ours
use-theirs = Use theirs
mark-resolved = Mark as resolved
open-merge-tool = Open in merge tool
confirm-resolved = { $count ->
    [one] This file still contains conflict markers. Mark it as resolved anyway?
   *[other] These { $count } files still contain conflict markers. Mark them as resolved anyway?
}
operation-merge = Merging { $name }
operation-merging = Merging
operation-rebase-step = Rebasing { $step }/{ $total }
operation-rebasing = Rebasing
operation-cherry-pick = Cherry-picking { $commit }
operation-cherry-picking = Cherry-picking
operation-revert = Reverting { $commit }
operation-reverting = Reverting
operation-bisect = Bisecting
operation-apply = Applying patches
operation-apply-step = Applying patches { $step }/{ $total }
operation-conflicts = { $operation } — { $count ->
    [one] 1 conflict
   *[other] { $count } conflicts
}
skip = Skip
abort = Abort
end-bisect = End bisect
confirm-abort = Abort the operation in progress? The repository will return to its previous state.
reset = Reset
reset-soft = Soft
reset-mixed = Mixed
reset-hard = Hard
reset-soft-hint = Move the branch only; keep the index and the working tree.
reset-mixed-hint = Move the branch and unstage its changes; keep the working tree.
reset-hard-hint = Move the branch and discard all changes in the index and the working tree.
reset-hard-warning = Uncommitted changes will be lost.
confirm-delete-branch = Delete branch { $name }?
confirm-delete-tag = Delete tag { $name }?

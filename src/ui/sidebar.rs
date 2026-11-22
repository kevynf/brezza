use egui::Ui;

use super::{icons, time_fmt};
use crate::fl;
use crate::git::protocol::Command;
use crate::model::{History, RefInfo, RefKind, StashInfo};
use crate::tab::{Dialog, NameAction, RepoTab, Selection};
use crate::ui::chrome::{self, Row};

enum Action {
    Reveal(crate::model::Oid),
    Run(Command),
    Integrate(Command),
    PushTag(String),
    Dialog(Dialog),
}

pub fn show(ui: &mut Ui, tab: &mut RepoTab) {
    let Some(history) = &tab.history else {
        return;
    };
    let head = tab.head_branch().map(str::to_owned);
    let selection = tab.selection;
    let mut action = None;
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            chrome::sidebar(ui, |ui| {
                for (kind, title) in [
                    (RefKind::Local, fl!("branches")),
                    (RefKind::Remote, fl!("remotes")),
                    (RefKind::Tag, fl!("tags")),
                ] {
                    let refs: Vec<&RefInfo> =
                        history.refs.iter().filter(|r| r.kind == kind).collect();
                    let id = ui.make_persistent_id(("section", kind as u8));
                    let open =
                        chrome::group(ui, id, &title, refs.len(), kind != RefKind::Tag, |_| {});
                    if open {
                        let items = refs.iter().map(|r| (r.name.as_str(), *r)).collect();
                        chrome::tree(ui, id, items, 0, &mut |ui, depth, label, r| {
                            ref_row(ui, depth, label, r, head.as_deref(), history, &mut action)
                        });
                    }
                }
                let id = ui.make_persistent_id(("section", "stashes"));
                let title = fl!("stashes");
                if chrome::group(ui, id, &title, history.stashes.len(), false, |_| {}) {
                    for (index, stash) in history.stashes.iter().enumerate() {
                        stash_row(ui, index, stash, selection, &mut action);
                    }
                }
            });
        });
    match action {
        Some(Action::Reveal(id)) => tab.reveal(id),
        Some(Action::Run(command)) => tab.run(command),
        Some(Action::Integrate(command)) => tab.integrate(command),
        Some(Action::PushTag(tag)) => tab.push_tag(&tag),
        Some(Action::Dialog(dialog)) => tab.dialog = Some(dialog),
        None => {}
    }
}

fn ref_row(
    ui: &mut Ui,
    depth: usize,
    label: &str,
    r: &RefInfo,
    head: Option<&str>,
    history: &History,
    action: &mut Option<Action>,
) {
    let is_head = r.kind == RefKind::Local && head == Some(r.name.as_str());
    let icon = match r.kind {
        RefKind::Local => icons::GIT_BRANCH,
        RefKind::Remote => icons::CLOUD,
        RefKind::Tag => icons::TAG,
    };
    let row = Row {
        depth,
        icon,
        label,
        active: is_head,
        ..Row::default()
    };
    let response = chrome::nav_row(ui, row, |ui| {
        if let Some(upstream) = &r.upstream {
            chrome::divergence(ui, upstream);
        }
    })
    .on_hover_text(r.name.as_str());
    if response.clicked() {
        *action = Some(Action::Reveal(r.target));
    }
    if response.double_clicked() && r.kind != RefKind::Tag {
        *action = Some(Action::Run(checkout(r, history)));
    }
    let remote = history.remote_branch(&r.name);
    chrome::context_menu(&response, |ui| {
        if r.kind != RefKind::Tag && chrome::menu_item(ui, fl!("checkout")).clicked() {
            *action = Some(Action::Run(checkout(r, history)));
        }
        if chrome::menu_item(ui, fl!("create-branch")).clicked() {
            let value = match r.kind {
                RefKind::Remote => remote.map_or_else(String::new, |(_, branch)| branch.to_owned()),
                _ => String::new(),
            };
            *action = Some(Action::Dialog(Dialog::Name {
                title: fl!("branch-name"),
                value,
                action: NameAction::CreateBranch(r.target),
            }));
        }
        if r.kind == RefKind::Local && chrome::menu_item(ui, fl!("rename-branch")).clicked() {
            *action = Some(Action::Dialog(Dialog::Name {
                title: fl!("rename-branch-title"),
                value: r.name.clone(),
                action: NameAction::RenameBranch(r.name.clone()),
            }));
        }
        if r.kind == RefKind::Local
            && r.upstream.is_some()
            && chrome::menu_item(ui, fl!("unset-upstream")).clicked()
        {
            *action = Some(Action::Run(Command::UnsetUpstream(r.name.clone())));
        }
        if r.kind == RefKind::Remote
            && head.is_some()
            && let Some((remote, branch)) = remote
            && chrome::menu_item(ui, fl!("set-upstream")).clicked()
        {
            *action = Some(Action::Run(Command::SetUpstream {
                remote: remote.to_owned(),
                branch: branch.to_owned(),
            }));
        }
        if r.kind != RefKind::Tag && head.is_some() && !(r.kind == RefKind::Local && is_head) {
            chrome::menu_separator(ui);
            if chrome::menu_item(ui, fl!("merge-into-current")).clicked() {
                *action = Some(Action::Integrate(Command::Merge {
                    target: r.name.clone(),
                    no_ff: false,
                    autostash: false,
                }));
            }
            if chrome::menu_item(ui, fl!("merge-no-fast-forward")).clicked() {
                *action = Some(Action::Integrate(Command::Merge {
                    target: r.name.clone(),
                    no_ff: true,
                    autostash: false,
                }));
            }
            if chrome::menu_item(ui, fl!("rebase-onto")).clicked() {
                *action = Some(Action::Integrate(Command::Rebase {
                    target: r.name.clone(),
                    autostash: false,
                }));
            }
        }
        if r.kind == RefKind::Local && !is_head {
            chrome::menu_separator(ui);
            if chrome::menu_item(ui, fl!("delete-branch")).clicked() {
                let text = fl!("confirm-delete-branch", name = r.name.as_str());
                *action = Some(Action::Dialog(Dialog::Confirm {
                    text,
                    icon: icons::TRASH_2,
                    button: fl!("delete-branch"),
                    danger: true,
                    command: Command::DeleteBranch {
                        name: r.name.clone(),
                        force: false,
                    },
                }));
            }
        }
        if r.kind == RefKind::Tag {
            chrome::menu_separator(ui);
            if chrome::menu_item(ui, fl!("delete-tag")).clicked() {
                let text = fl!("confirm-delete-tag", name = r.name.as_str());
                *action = Some(Action::Dialog(Dialog::Confirm {
                    text,
                    icon: icons::TRASH_2,
                    button: fl!("delete-tag"),
                    danger: true,
                    command: Command::DeleteTag(r.name.clone()),
                }));
            }
            if chrome::menu_item(ui, fl!("push-tag")).clicked() {
                *action = Some(Action::PushTag(r.name.clone()));
            }
        }
        if r.kind == RefKind::Remote {
            chrome::menu_separator(ui);
            if let Some((remote, branch)) = remote
                && chrome::menu_item(ui, fl!("delete-remote-branch")).clicked()
            {
                let command = Command::DeleteRemoteBranch {
                    remote: remote.to_owned(),
                    branch: branch.to_owned(),
                };
                let text = fl!("confirm-delete-remote", remote = remote, branch = branch);
                *action = Some(Action::Dialog(Dialog::Confirm {
                    text,
                    icon: icons::TRASH_2,
                    button: fl!("delete-remote-branch"),
                    danger: true,
                    command,
                }));
            }
        }
        chrome::menu_separator(ui);
        if chrome::menu_item(ui, fl!("copy-name")).clicked() {
            ui.ctx().copy_text(r.name.clone());
        }
    });
}

fn stash_row(
    ui: &mut Ui,
    index: usize,
    stash: &StashInfo,
    selection: Selection,
    action: &mut Option<Action>,
) {
    let time = time_fmt::short(stash.time);
    let row = Row {
        icon: icons::ARCHIVE,
        label: stash.summary(),
        detail: Some(&time),
        selected: selection == Selection::Commit(stash.id),
        ..Row::default()
    };
    let response = chrome::nav_row(ui, row, |_| {}).on_hover_text(stash.message.as_str());
    if response.clicked() {
        *action = Some(Action::Reveal(stash.id));
    }
    chrome::context_menu(&response, |ui| {
        if chrome::menu_item(ui, fl!("stash-apply")).clicked() {
            *action = Some(Action::Run(Command::StashApply {
                index,
                id: stash.id,
            }));
        }
        if chrome::menu_item(ui, fl!("stash-pop")).clicked() {
            *action = Some(Action::Run(Command::StashPopAt {
                index,
                id: stash.id,
            }));
        }
        chrome::menu_separator(ui);
        if chrome::menu_item(ui, fl!("stash-drop")).clicked() {
            *action = Some(Action::Dialog(Dialog::Confirm {
                text: fl!("confirm-stash-drop"),
                icon: icons::TRASH_2,
                button: fl!("stash-drop"),
                danger: true,
                command: Command::StashDrop {
                    index,
                    id: stash.id,
                },
            }));
        }
        chrome::menu_separator(ui);
        if chrome::menu_item(ui, fl!("copy-hash")).clicked() {
            ui.ctx().copy_text(stash.id.to_string());
        }
    });
}

fn checkout(r: &RefInfo, history: &History) -> Command {
    if r.kind != RefKind::Remote {
        return Command::Checkout(r.name.clone());
    }
    let Some((remote, branch)) = history.remote_branch(&r.name) else {
        return Command::Checkout(r.name.clone());
    };
    if history.local(branch).is_some() {
        Command::Checkout(branch.to_owned())
    } else {
        Command::TrackBranch {
            branch: branch.to_owned(),
            remote: remote.to_owned(),
        }
    }
}

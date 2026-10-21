//! Finding projects: the git repositories in the projects folders, with their
//! worktrees and submodules, named as `tms` (jrmoulton/tmux-sessionizer)
//! names its tmux sessions, so the menu and `tms` land in the same ones.
//!
//! `tms`'s rules, from its `repos.rs` and `session.rs`, with `list_worktrees`
//! and `search_submodules` on:
//!
//! - A folder is searched breadth first, down to its depth. A repository stops the search: nothing
//!   inside it is searched.
//! - A repository is a folder with a `.git` entry, or a bare one: a `HEAD` file and `objects` and
//!   `refs` folders.
//! - A linked worktree found by the search is left out; it shows under its repository instead, as
//!   `<repository>#<worktree id>`.
//! - A repository is named after its folder. Names found more than once take as many parent folders
//!   as tell them apart (`to/proj2/test`).
//! - A checked-out submodule is `<repository>>`, then its folder's name.
//! - The tmux session is the name with every `.` made `_`.
//!
//! # Owns
//!
//! - The search of the projects folders, and the folders whose changes can add or remove a project.
//! - Repositories, worktrees and submodules, and their names.
//!
//! # Not here
//!
//! - Watching the folders and searching again: `scan.rs`.
//! - Opening a project: `main.rs`, with the commands from `settings.rs`.

use std::collections::BTreeMap;
use std::collections::HashSet;
use std::collections::VecDeque;
use std::ffi::OsString;
use std::fs;
use std::path::Component;
use std::path::Path;
use std::path::PathBuf;

/// A folder to find projects in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectFolder {
    pub path: PathBuf,
    /// How many levels below `path` a repository may be; 0 checks only
    /// `path` itself.
    pub depth: usize,
}

/// One project: a repository, a worktree or a submodule.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Project {
    /// What `tms` shows, such as `kotodama`, `kotodama#main` or `app>lib`.
    pub name: String,
    pub path: PathBuf,
}

impl Project {
    /// The tmux session name: tmux won't take a `.` in one.
    pub fn session(&self) -> String {
        self.name.replace('.', "_")
    }
}

/// What a search found.
#[derive(Debug, Default)]
pub struct Found {
    pub projects: Vec<Project>,
    /// The folders whose changes can add or remove a project: those searched,
    /// and each repository's own, where worktrees and submodules appear.
    pub dirs: Vec<PathBuf>,
}

/// Finds the projects in `folders`, which are searched together, as `tms`
/// searches its search directories, so a name found in two of them is told
/// apart.
pub fn find(folders: &[ProjectFolder]) -> Found {
    let mut found = Found::default();
    for (name, mut repositories) in search(folders, &mut found) {
        let named: Vec<Project> = if repositories.len() == 1 {
            vec![Project {
                name,
                path: repositories.pop().unwrap_or_default(),
            }]
        } else {
            tell_apart(repositories)
        };
        for project in named {
            submodules(&project, &mut found);
            found.projects.push(project);
        }
    }
    found
}

/// Searches `folders` breadth first for repositories and their worktrees, adding the folders it
/// goes through to `found`. Returns their paths by name, in the order found, as `tms` groups them
/// before telling equal names apart.
fn search(folders: &[ProjectFolder], found: &mut Found) -> BTreeMap<String, Vec<PathBuf>> {
    let mut seen = HashSet::new();
    let mut by_name: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();
    let mut queue: VecDeque<(PathBuf, usize)> = folders
        .iter()
        .map(|folder| (folder.path.clone(), folder.depth))
        .collect();
    while let Some((path, depth)) = queue.pop_front() {
        let Ok(entries) = fs::read_dir(&path) else {
            continue;
        };
        let mut names: Vec<(OsString, Kind)> = entries
            .flatten()
            .filter_map(|entry| {
                let kind = match entry.file_type().ok()? {
                    kind if kind.is_dir() => Kind::Dir,
                    kind if kind.is_file() => Kind::File,
                    _ => Kind::Other,
                };
                Some((entry.file_name(), kind))
            })
            .collect();
        names.sort();
        if let Some(git_dir) = git_dir(&path, &names) {
            found.dirs.push(path.clone());
            if !seen.insert(path.clone()) || is_linked_worktree(&path, &git_dir) {
                continue;
            }
            let Some(name) = path.file_name() else {
                continue;
            };
            let name = name.to_string_lossy().into_owned();
            for (id, worktree) in worktrees(&git_dir) {
                found.dirs.push(worktree.clone());
                by_name.entry(format!("{name}#{id}")).or_default().push(worktree);
            }
            by_name.entry(name).or_default().push(path);
            continue;
        }
        found.dirs.push(path.clone());
        if depth == 0 {
            continue;
        }
        for (name, kind) in names {
            let child = path.join(name);
            // A link to a folder is searched, as `tms` follows links; the
            // depth bounds a loop.
            if kind == Kind::Dir || (kind == Kind::Other && child.is_dir()) {
                queue.push_back((child, depth - 1));
            }
        }
    }
    by_name
}

/// What a folder entry is, without following a link.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Kind {
    Dir,
    File,
    Other,
}

/// Whether `path`, whose entries are `names`, is a repository, as `tms`
/// tells, and if so its git dir: the one its `.git` names, or for a bare one
/// the folder itself.
fn git_dir(path: &Path, names: &[(OsString, Kind)]) -> Option<PathBuf> {
    if names.iter().any(|(name, _)| name == ".git") {
        return Some(dot_git(path));
    }
    let has = |want: &str, kind: Kind| names.iter().any(|(name, k)| name == want && *k == kind);
    (has("HEAD", Kind::File) && has("objects", Kind::Dir) && has("refs", Kind::Dir)).then(|| path.to_owned())
}

/// The git dir a repository's `.git` names: the folder itself, or the one a
/// `gitdir:` file points at.
fn dot_git(path: &Path) -> PathBuf {
    let dot_git = path.join(".git");
    match fs::read_to_string(&dot_git) {
        Ok(text) => match text.lines().next().and_then(|line| line.strip_prefix("gitdir:")) {
            Some(target) => normalize(&path.join(target.trim())),
            None => dot_git,
        },
        Err(_) => dot_git,
    }
}

/// A linked worktree's `.git` is a file naming a git dir with a `commondir`,
/// which leads back to its repository's. A submodule's, or one pointing at a
/// bare repository, has none.
fn is_linked_worktree(path: &Path, git_dir: &Path) -> bool {
    path.join(".git").is_file() && git_dir.join("commondir").is_file()
}

/// The worktrees of the repository whose git dir is `git_dir`, by id, in id
/// order. One whose folder is gone is left out.
fn worktrees(git_dir: &Path) -> Vec<(String, PathBuf)> {
    let Ok(entries) = fs::read_dir(git_dir.join("worktrees")) else {
        return Vec::new();
    };
    let mut worktrees: Vec<(String, PathBuf)> = entries
        .flatten()
        .filter_map(|entry| {
            let private = entry.path();
            let named = fs::read_to_string(private.join("gitdir")).ok()?;
            // `gitdir` names the worktree's `.git` file, absolute or relative
            // to the worktree's private git dir.
            let dot_git = normalize(&private.join(named.trim()));
            let worktree = dot_git.parent()?.to_owned();
            worktree
                .join(".git")
                .exists()
                .then(|| (entry.file_name().to_string_lossy().into_owned(), worktree))
        })
        .collect();
    worktrees.sort();
    worktrees
}

/// Names equal names apart as `tms`'s `deduplicate_sessions` does, so the
/// sessions match even where its rule is odd: every name takes the same
/// number of folders, enough to tell apart the pair that needs most, as
/// `tms` counts it.
fn tell_apart(mut paths: Vec<PathBuf>) -> Vec<Project> {
    let mut depth = 1;
    let mut popped = Vec::with_capacity(paths.len());
    while let Some(current) = paths.pop() {
        let mut current_depth = 1;
        let mut equal = true;
        while equal {
            equal = false;
            if let Some(part) = current.iter().rev().nth(current_depth) {
                for other in &paths {
                    if other.iter().rev().nth(current_depth) == Some(part) {
                        current_depth += 1;
                        equal = true;
                        break;
                    }
                }
            }
        }
        depth = depth.max(current_depth);
        popped.push(current);
    }
    popped
        .into_iter()
        .map(|path| {
            let mut parts: Vec<String> = path
                .iter()
                .rev()
                .take(depth + 1)
                .map(|part| part.to_string_lossy().into_owned())
                .collect();
            parts.reverse();
            Project {
                name: parts.join("/"),
                path,
            }
        })
        .collect()
}

/// Adds the checked-out submodules of `project`, and theirs, named after it.
fn submodules(project: &Project, found: &mut Found) {
    let Ok(text) = fs::read_to_string(project.path.join(".gitmodules")) else {
        return;
    };
    for path in submodule_paths(&text) {
        let path = project.path.join(path);
        found.dirs.push(path.clone());
        if !path.join(".git").exists() {
            continue;
        }
        let Some(folder) = path.file_name() else {
            continue;
        };
        let submodule = Project {
            name: format!("{}>{}", project.name, folder.to_string_lossy()),
            path,
        };
        submodules(&submodule, found);
        found.projects.push(submodule);
    }
}

/// The `path`s in a `.gitmodules` file.
fn submodule_paths(text: &str) -> Vec<&str> {
    let mut paths = Vec::new();
    let mut in_submodule = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_submodule = line.starts_with("[submodule");
            continue;
        }
        if !in_submodule {
            continue;
        }
        if let Some((key, value)) = line.split_once('=')
            && key.trim().eq_ignore_ascii_case("path")
        {
            let value = value.trim();
            let value = value
                .strip_prefix('"')
                .and_then(|value| value.strip_suffix('"'))
                .unwrap_or(value);
            if !value.is_empty() {
                paths.push(value);
            }
        }
    }
    paths
}

/// `path` without `.` and with each `..` taking the folder before it, as
/// git resolves a relative `gitdir`.
fn normalize(path: &Path) -> PathBuf {
    let mut normal = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normal.pop();
            }
            other => normal.push(other),
        }
    }
    normal
}

#[cfg(test)]
mod tests;

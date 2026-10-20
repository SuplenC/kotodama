//! Finding projects and naming their tmux sessions as `tms`
//! (jrmoulton/tmux-sessionizer) does, so the menu and `tms` land in the same
//! sessions. The cases follow `tms`'s own tests (`tests/repos.rs` and
//! `src/session.rs`), with `list_worktrees` and `search_submodules` on. Needs
//! `git`, which the dev environment provides.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use kotodama_folders::projects::{self, Project, ProjectFolder};

fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(
        status.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&status.stderr)
    );
}

/// A repository with one commit at `dir`.
fn repo(dir: &Path) {
    fs::create_dir_all(dir).unwrap();
    git(dir, &["init", "-q", "-b", "main"]);
    git(dir, &["commit", "-q", "--allow-empty", "-m", "init"]);
}

/// A folder to search, canonical so paths compare equal.
fn search() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let search = fs::canonicalize(dir.path()).unwrap();
    (dir, search)
}

fn find(search: &Path, depth: usize) -> Vec<Project> {
    projects::find(&[ProjectFolder {
        path: search.to_owned(),
        depth,
    }])
    .projects
}

/// The names found, sorted.
fn names(projects: &[Project]) -> Vec<String> {
    let mut names: Vec<String> = projects.iter().map(|p| p.name.clone()).collect();
    names.sort();
    names
}

fn named<'a>(projects: &'a [Project], name: &str) -> &'a Project {
    projects
        .iter()
        .find(|p| p.name == name)
        .unwrap_or_else(|| panic!("no project named {name} in {:?}", names(projects)))
}

#[test]
fn a_repository_is_named_after_its_folder() {
    let (_dir, search) = search();
    repo(&search.join("kotodama"));

    let found = find(&search, 2);
    assert_eq!(names(&found), ["kotodama"]);
    assert_eq!(found[0].path, search.join("kotodama"));
    assert_eq!(found[0].session(), "kotodama");
}

#[test]
fn dots_become_underscores_in_the_session_but_not_the_name() {
    let (_dir, search) = search();
    repo(&search.join("kotodama.nvim"));
    repo(&search.join("v1.2.3"));

    let found = find(&search, 2);
    assert_eq!(named(&found, "kotodama.nvim").session(), "kotodama_nvim");
    assert_eq!(named(&found, "v1.2.3").session(), "v1_2_3");
}

#[test]
fn spaces_and_other_characters_stay_as_they_are() {
    let (_dir, search) = search();
    repo(&search.join("my project"));
    repo(&search.join("a:b"));

    let found = find(&search, 2);
    assert_eq!(named(&found, "my project").session(), "my project");
    assert_eq!(named(&found, "a:b").session(), "a:b");
}

#[test]
fn repositories_are_found_down_to_the_depth_and_not_inside_another() {
    let (_dir, search) = search();
    repo(&search.join("top"));
    repo(&search.join("top/nested"));
    repo(&search.join("work/api"));
    repo(&search.join("work/deep/too-deep"));

    assert_eq!(names(&find(&search, 2)), ["api", "top"]);
    assert_eq!(names(&find(&search, 3)), ["api", "too-deep", "top"]);
    assert_eq!(names(&find(&search, 0)), Vec::<String>::new());
}

#[test]
fn the_folder_itself_can_be_the_repository() {
    let (_dir, search) = search();
    repo(&search.join("solo"));

    assert_eq!(names(&find(&search.join("solo"), 0)), ["solo"]);
}

#[test]
fn a_gitlink_to_a_bare_repository_is_a_project() {
    let (_dir, search) = search();
    let project = search.join("my-project");
    fs::create_dir_all(&project).unwrap();
    git(&project, &["init", "-q", "--bare", ".bare"]);
    fs::write(project.join(".git"), "gitdir: .bare\n").unwrap();

    assert_eq!(names(&find(&search, 2)), ["my-project"]);
}

#[test]
fn worktrees_of_a_bare_repository_are_named_after_it() {
    let (_dir, search) = search();
    let project = search.join("my-project");
    fs::create_dir_all(&project).unwrap();
    git(&project, &["init", "-q", "--bare"]);
    git(&project, &["worktree", "add", "-q", "main"]);
    git(&project, &["worktree", "add", "-q", "test"]);

    let found = find(&search, 2);
    assert_eq!(
        names(&found),
        ["my-project", "my-project#main", "my-project#test"]
    );
    assert_eq!(named(&found, "my-project#main").path, project.join("main"));
}

#[test]
fn worktrees_with_relative_paths_are_found_too() {
    let (_dir, search) = search();
    let project = search.join("my-project");
    fs::create_dir_all(&project).unwrap();
    git(&project, &["init", "-q", "--bare"]);
    git(&project, &["config", "worktree.useRelativePaths", "true"]);
    git(&project, &["worktree", "add", "-q", "main"]);

    let found = find(&search, 2);
    assert_eq!(names(&found), ["my-project", "my-project#main"]);
    assert_eq!(named(&found, "my-project#main").path, project.join("main"));
}

#[test]
fn a_linked_worktree_shows_only_under_its_repository() {
    let (_dir, search) = search();
    let main = search.join("main-repo");
    repo(&main);
    git(
        &main,
        &["worktree", "add", "-q", "-b", "linked", "../linked"],
    );

    let found = find(&search, 2);
    assert_eq!(names(&found), ["main-repo", "main-repo#linked"]);
    assert_eq!(
        named(&found, "main-repo#linked").path,
        search.join("linked")
    );
}

#[test]
fn a_removed_worktree_is_left_out() {
    let (_dir, search) = search();
    let main = search.join("app");
    repo(&main);
    git(&main, &["worktree", "add", "-q", "-b", "gone", "../gone"]);
    fs::remove_dir_all(search.join("gone")).unwrap();

    assert_eq!(names(&find(&search, 2)), ["app"]);
}

#[test]
fn the_same_name_twice_takes_as_many_parent_folders_as_tell_them_apart() {
    // tms's `verify_session_name_deduplication`.
    let (_dir, search) = search();
    repo(&search.join("search/path/to/proj1/test"));
    repo(&search.join("search/path/to/proj2/test"));
    repo(&search.join("other/path/to/projects/proj2/test"));

    let found = find(&search, 6);
    assert_eq!(
        names(&found),
        ["projects/proj2/test", "to/proj1/test", "to/proj2/test"]
    );
    assert_eq!(
        named(&found, "to/proj1/test").path,
        search.join("search/path/to/proj1/test")
    );
}

#[test]
fn a_session_name_keeps_the_slashes_of_a_told_apart_name_and_drops_dots() {
    let (_dir, search) = search();
    repo(&search.join("a.b/app"));
    repo(&search.join("c/app"));

    let found = find(&search, 2);
    assert_eq!(names(&found), ["a.b/app", "c/app"]);
    assert_eq!(named(&found, "a.b/app").session(), "a_b/app");
}

#[test]
fn submodules_are_named_after_their_repository() {
    let (_dir, search) = search();
    let library = search.join("outside/lib.rs");
    repo(&library);
    let app = search.join("projects/app");
    repo(&app);
    git(
        &app,
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "--quiet",
            "add",
            library.to_str().unwrap(),
            "vendor/lib.rs",
        ],
    );

    let found = find(&search.join("projects"), 2);
    assert_eq!(names(&found), ["app", "app>lib.rs"]);
    let submodule = named(&found, "app>lib.rs");
    assert_eq!(submodule.path, app.join("vendor/lib.rs"));
    assert_eq!(submodule.session(), "app>lib_rs");
}

#[test]
fn a_submodule_not_checked_out_is_left_out() {
    let (_dir, search) = search();
    let app = search.join("app");
    repo(&app);
    fs::write(
        app.join(".gitmodules"),
        "[submodule \"lib\"]\n\tpath = lib\n\turl = https://example.org/lib.git\n",
    )
    .unwrap();
    fs::create_dir_all(app.join("lib")).unwrap();

    assert_eq!(names(&find(&search, 2)), ["app"]);
}

#[test]
fn folders_are_searched_together_so_a_name_in_both_is_told_apart() {
    let (_dir, search) = search();
    repo(&search.join("work/api"));
    repo(&search.join("home/api"));

    let found = projects::find(&[
        ProjectFolder {
            path: search.join("work"),
            depth: 1,
        },
        ProjectFolder {
            path: search.join("home"),
            depth: 1,
        },
    ])
    .projects;
    assert_eq!(names(&found), ["home/api", "work/api"]);
}

#[test]
fn a_folder_that_doesnt_exist_finds_nothing() {
    let (_dir, search) = search();
    assert!(find(&search.join("missing"), 2).is_empty());
}

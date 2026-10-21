//! Git repositories for the tests to find, made with the `git` the dev environment provides, under
//! a fixed identity and without the user's or the system's git config. Shared by every integration
//! test, each of which includes it with `#[path]`: a file under `tests/` would be a test target of
//! its own, and a `mod.rs` is not used.

use std::fs;
use std::path::Path;
use std::process::Command;

/// Runs `git` with `args` in `dir`, and panics with its stderr if it fails.
pub fn git(dir: &Path, args: &[&str]) {
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
pub fn repo(dir: &Path) {
    fs::create_dir_all(dir).unwrap();
    git(dir, &["init", "-q", "-b", "main"]);
    git(dir, &["commit", "-q", "--allow-empty", "-m", "init"]);
}

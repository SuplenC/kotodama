//! Runs `kotodama extension check` on the example Extensions, on broken ones, and on each
//! scaffold `kotodama extension new` writes. The examples and scaffolds need bash, jq, Python
//! and Node 23.6 or later, and the Rust scaffold cargo, all of which the dev environment
//! provides.

use std::env;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::Output;

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Runs `kotodama` in the repository, where the examples' paths are relative ones, as an
/// author types them.
fn kotodama(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_kotodama"))
        .args(args)
        .current_dir(repo())
        .output()
        .unwrap()
}

/// The report, with the exit status and the Extension's own stderr to read when it fails.
fn report(output: &Output) -> String {
    format!(
        "{}\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn passes(args: &[&str]) {
    let output = kotodama(args);
    let report = report(&output);
    assert!(output.status.success(), "{report}");
    assert!(report.contains("\nNo problems."), "{report}");
}

fn example(name: &str) -> String {
    format!("examples/{name}")
}

#[test]
fn the_shell_example_passes_the_check() {
    passes(&["extension", "check", &example("shell-extension")]);
}

#[test]
fn the_python_example_passes_the_check_with_a_query_and_an_action() {
    passes(&[
        "extension",
        "check",
        &example("python-extension"),
        "--query",
        "hello world",
        "--run",
        "upper:hello world",
    ]);
}

#[test]
fn the_typescript_example_passes_the_check_with_a_query_and_an_action() {
    passes(&[
        "extension",
        "check",
        &example("typescript-extension"),
        "--query",
        "0",
        "--run",
        "1970-01-01T00:00:00.000Z",
    ]);
}

/// An Extension in `dir` whose program is the bash script `script`, after a helper that sends
/// its argument as a frame.
fn broken(dir: &Path, script: &str) {
    fs::write(
        dir.join("manifest.toml"),
        "name = \"broken\"\ncommand = [\"./extension.sh\"]\n",
    )
    .unwrap();
    let program = dir.join("extension.sh");
    let send = r#"send() { printf '%d\n%s' "${#1}" "$1"; }"#;
    fs::write(
        &program,
        format!("#!/usr/bin/env bash\nexport LC_ALL=C\n{send}\n{script}\n"),
    )
    .unwrap();
    fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
}

fn fails(dir: &Path, expected: &str) {
    let output = kotodama(&["extension", "check", dir.to_str().unwrap()]);
    let report = report(&output);
    assert_eq!(output.status.code(), Some(1), "{report}");
    assert!(report.contains(expected), "{report}");
}

#[test]
fn a_log_line_on_stdout_is_explained() {
    let dir = tempfile::tempdir().unwrap();
    broken(dir.path(), "echo starting up\ncat >/dev/null");
    fails(dir.path(), "goes to stderr");
}

#[test]
fn an_item_without_actions_is_explained() {
    let dir = tempfile::tempdir().unwrap();
    broken(
        dir.path(),
        r#"send '{"version":1,"minor":0,"formats":["json"]}'
send '{"add_items":{"items":[{"id":"a","kind":"broken.thing","title":"A","actions":[]}]}}'
cat >/dev/null"#,
    );
    fails(dir.path(), "Item `a` has no Actions");
}

#[test]
fn an_extension_that_outlives_its_stdin_is_warned_about() {
    let dir = tempfile::tempdir().unwrap();
    broken(
        dir.path(),
        r#"send '{"version":1,"minor":0,"formats":["json"]}'
send '{"add_items":{"items":[{"id":"a","kind":"broken.thing","title":"A","actions":[{"id":"o","title":"O"}]}]}}'
cat >/dev/null
sleep 10"#,
    );
    let output = kotodama(&["extension", "check", dir.path().to_str().unwrap()]);
    let report = report(&output);
    assert!(output.status.success(), "{report}");
    assert!(report.contains("still running"), "{report}");
}

/// Writes the scaffold `notes` in `language` under `parent`, and returns its directory.
fn scaffold(parent: &Path, language: &str) -> String {
    let parent = parent.to_str().unwrap();
    let output = kotodama(&["extension", "new", "notes", "--lang", language, "--dir", parent]);
    assert!(output.status.success(), "{}", report(&output));
    format!("{parent}/notes")
}

#[test]
fn the_shell_scaffold_passes_the_check() {
    let parent = tempfile::tempdir().unwrap();
    let dir = scaffold(parent.path(), "shell");
    passes(&["extension", "check", &dir, "--run", "hello"]);
}

#[test]
fn the_python_scaffold_passes_the_check() {
    let parent = tempfile::tempdir().unwrap();
    let dir = scaffold(parent.path(), "python");
    passes(&["extension", "check", &dir, "--query", "hi there", "--run", "hi there"]);
}

#[test]
fn the_rust_scaffold_builds_and_passes_the_check() {
    let parent = tempfile::tempdir().unwrap();
    let dir = scaffold(parent.path(), "rust");
    // The scaffold depends on the crates in the Kotodama repository on GitHub. This test builds
    // it against the ones in this checkout instead, offline, and in this workspace's target
    // directory, whose build of them it reuses.
    let crates = repo().join("crates");
    let manifest = Path::new(&dir).join("Cargo.toml");
    let cargo_toml = fs::read_to_string(&manifest).unwrap();
    let mut local = cargo_toml.clone();
    for name in ["kotodama-extension", "kotodama-protocol"] {
        local = local.replace(
            &format!(r#"{name} = {{ git = "https://github.com/SuplenC/kotodama" }}"#),
            &format!("{name} = {{ path = {:?} }}", crates.join(name)),
        );
    }
    assert_ne!(local, cargo_toml, "the scaffold's dependencies moved");
    fs::write(&manifest, local).unwrap();

    let target = env::var_os("CARGO_TARGET_DIR").map_or_else(
        || env::current_exe().unwrap().ancestors().nth(3).unwrap().to_owned(),
        PathBuf::from,
    );
    let built = Command::new(env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
        .args([
            "install",
            "--path",
            &dir,
            "--root",
            &dir,
            "--debug",
            "--offline",
            "--quiet",
        ])
        .env("CARGO_TARGET_DIR", target)
        .output()
        .unwrap();
    assert!(built.status.success(), "{}", report(&built));
    passes(&["extension", "check", &dir, "--run", "hello"]);
}

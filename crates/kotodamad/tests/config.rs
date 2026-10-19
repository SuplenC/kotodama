//! The config directory: every top-level `*.toml` in the distro's directories,
//! then the user's, merged in order.

use std::fs;
use std::path::{Path, PathBuf};

use kotodama_protocol::{Table, Value};
use kotodamad::config;
use tempfile::TempDir;

struct Dirs {
    _root: TempDir,
    distro: PathBuf,
    user: PathBuf,
}

impl Dirs {
    fn new() -> Self {
        let root = TempDir::new().unwrap();
        let distro = root.path().join("etc/kotodama");
        let user = root.path().join("home/kotodama");
        fs::create_dir_all(&distro).unwrap();
        fs::create_dir_all(&user).unwrap();
        Dirs {
            _root: root,
            distro,
            user,
        }
    }

    fn load(&self) -> Result<Table, config::Error> {
        config::load(&[self.distro.clone(), self.user.clone()])
    }
}

fn write(dir: &Path, name: &str, toml: &str) {
    fs::write(dir.join(name), toml).unwrap();
}

fn get<'a>(table: &'a Table, path: &str) -> Option<&'a Value> {
    let mut parts = path.split('.');
    let mut value = table.get(parts.next()?)?;
    for part in parts {
        let Value::Table(table) = value else {
            return None;
        };
        value = table.get(part)?;
    }
    Some(value)
}

fn string(value: &str) -> Value {
    Value::String(value.into())
}

#[test]
fn the_user_directory_wins_over_the_distro_directory() {
    let dirs = Dirs::new();
    write(
        &dirs.distro,
        "defaults.toml",
        "theme = \"distro\"\nwidth = 600",
    );
    write(&dirs.user, "mine.toml", "theme = \"user\"");

    let config = dirs.load().unwrap();
    assert_eq!(get(&config, "theme"), Some(&string("user")));
    assert_eq!(get(&config, "width"), Some(&Value::Integer(600)));
}

#[test]
fn files_in_one_directory_are_read_in_filename_order() {
    let dirs = Dirs::new();
    write(&dirs.user, "20-later.toml", "theme = \"twenty\"");
    write(&dirs.user, "10-first.toml", "theme = \"ten\"");

    assert_eq!(get(&dirs.load().unwrap(), "theme"), Some(&string("twenty")));
}

#[test]
fn tables_merge_key_by_key() {
    let dirs = Dirs::new();
    write(
        &dirs.distro,
        "a.toml",
        "[extensions.apps]\nterminal = \"foot\"\nhidden = true",
    );
    write(
        &dirs.user,
        "b.toml",
        "[extensions.apps]\nterminal = \"kitty\"",
    );

    let config = dirs.load().unwrap();
    assert_eq!(
        get(&config, "extensions.apps.terminal"),
        Some(&string("kitty"))
    );
    assert_eq!(
        get(&config, "extensions.apps.hidden"),
        Some(&Value::Bool(true))
    );
}

#[test]
fn a_later_list_replaces_the_earlier_one_whole() {
    let dirs = Dirs::new();
    write(&dirs.distro, "a.toml", "roots = [\"~/src\", \"~/work\"]");
    write(&dirs.user, "b.toml", "roots = [\"~/code\"]");

    assert_eq!(
        get(&dirs.load().unwrap(), "roots"),
        Some(&Value::Array(vec![string("~/code")]))
    );
}

#[test]
fn subdirectories_and_other_files_are_ignored() {
    let dirs = Dirs::new();
    fs::create_dir_all(dirs.user.join("extensions/x")).unwrap();
    write(
        &dirs.user.join("extensions"),
        "nested.toml",
        "theme = \"nested\"",
    );
    write(&dirs.user, "notes.txt", "theme = \"text\"");
    write(&dirs.user, "real.toml", "theme = \"real\"");

    assert_eq!(get(&dirs.load().unwrap(), "theme"), Some(&string("real")));
}

#[test]
fn a_missing_directory_counts_as_empty() {
    let dirs = Dirs::new();
    write(&dirs.user, "a.toml", "theme = \"user\"");
    let config = config::load(&[dirs.distro.join("missing"), dirs.user.clone()]).unwrap();
    assert_eq!(get(&config, "theme"), Some(&string("user")));
}

#[test]
fn a_file_that_fails_to_parse_fails_the_whole_load_and_names_the_file() {
    let dirs = Dirs::new();
    write(&dirs.distro, "good.toml", "theme = \"distro\"");
    write(&dirs.user, "broken.toml", "theme = ");

    let error = dirs.load().unwrap_err();
    assert_eq!(error.path, dirs.user.join("broken.toml"));
}

use std::fs;
use std::fs::File;
use std::path::Path;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;
use std::time::SystemTime;

use kotodama_protocol::Table;
use kotodama_protocol::ToFrontEnd;
use kotodama_protocol::Value;
use tempfile::TempDir;

use super::Ask;
use super::CACHED_DIRS;
use super::Completer;
use super::Completions;
use super::Field;
use super::Job;
use super::Settings;
use super::serve;
use super::user_dirs;
use crate::daemon::Event;
use crate::daemon::FrontEndId;
use crate::problem::Report;

/// A home directory of its own, in the build's target directory rather than
/// the real home or `/tmp`.
fn home() -> TempDir {
    let scratch = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/scratch");
    fs::create_dir_all(&scratch).unwrap();
    tempfile::Builder::new().prefix("home-").tempdir_in(scratch).unwrap()
}

/// Makes `path` in `home`, a folder if it ends in `/`, modified `age`
/// seconds ago.
fn make(home: &Path, path: &str, age: u64) {
    let full = home.join(path.trim_end_matches('/'));
    if path.ends_with('/') {
        fs::create_dir_all(&full).unwrap();
    } else {
        fs::create_dir_all(full.parent().unwrap()).unwrap();
        File::create(&full).unwrap();
    }
    let at = SystemTime::now() - Duration::from_secs(age);
    File::open(&full).unwrap().set_modified(at).unwrap();
}

fn completer(home: &TempDir, recent: &[&str]) -> Completer {
    let recent_dirs = recent.iter().map(|dir| home.path().join(dir)).collect();
    Completer::new(
        Some(home.path().to_path_buf()),
        Settings {
            recent_dirs: Some(recent_dirs),
        },
    )
}

fn file(accept: &[&str]) -> Field {
    Field {
        accept: accept.iter().map(|&accept| accept.to_owned()).collect(),
        folder: false,
    }
}

fn titles(completer: &mut Completer, wanted: &Field, text: &str) -> Vec<String> {
    completer
        .complete(wanted, text)
        .into_iter()
        .map(|entry| entry.title)
        .collect()
}

#[test]
fn a_partial_path_completes_with_the_entries_of_its_directory_that_match() {
    let home = home();
    make(home.path(), "Downloads/", 0);
    make(home.path(), "Documents/", 0);
    make(home.path(), "dotfiles/", 0);
    let mut completer = completer(&home, &[]);

    let found = completer.complete(&file(&[]), "~/Do");

    let titles: Vec<&str> = found.iter().map(|entry| entry.title.as_str()).collect();
    assert_eq!(titles, ["Documents", "dotfiles", "Downloads"]);
    assert_eq!(found[0].path, home.path().join("Documents"));
    assert!(found[0].folder);
    assert_eq!(found[0].highlights, [0, 1]);
}

#[test]
fn a_relative_path_completes_from_the_home_directory() {
    let home = home();
    make(home.path(), "Downloads/work.conf", 0);
    let mut completer = completer(&home, &[]);

    assert_eq!(titles(&mut completer, &file(&[]), "Downloads/wo"), ["work.conf"]);
    assert_eq!(titles(&mut completer, &file(&[]), "Down"), ["Downloads"]);
}

#[test]
fn a_better_match_comes_first_and_folders_before_files_among_equal_ones() {
    let home = home();
    make(home.path(), "vpn/work.conf", 0);
    make(home.path(), "vpn/work/", 0);
    make(home.path(), "vpn/network/", 0);
    let mut completer = completer(&home, &[]);

    assert_eq!(
        titles(&mut completer, &file(&[]), "~/vpn/work"),
        ["work", "work.conf", "network"]
    );
}

#[test]
fn a_file_field_offers_folders_and_the_files_it_takes() {
    let home = home();
    make(home.path(), "vpn/work.conf", 0);
    make(home.path(), "vpn/notes.txt", 0);
    make(home.path(), "vpn/old/", 0);
    let mut completer = completer(&home, &[]);

    assert_eq!(titles(&mut completer, &file(&["conf"]), "~/vpn/"), ["old", "work.conf"]);
}

#[test]
fn a_folder_field_offers_only_folders() {
    let home = home();
    make(home.path(), "vpn/work.conf", 0);
    make(home.path(), "vpn/old/", 0);
    let mut completer = completer(&home, &[]);
    let folder = Field {
        accept: Vec::new(),
        folder: true,
    };

    assert_eq!(titles(&mut completer, &folder, "~/vpn/"), ["old"]);
}

#[test]
fn hidden_entries_show_only_once_a_dot_is_typed() {
    let home = home();
    make(home.path(), ".config/", 0);
    make(home.path(), "code/", 0);
    let mut completer = completer(&home, &[]);

    assert_eq!(titles(&mut completer, &file(&[]), "~/"), ["code"]);
    assert_eq!(titles(&mut completer, &file(&[]), "~/.c"), [".config"]);
}

#[test]
fn a_directory_is_read_again_only_once_it_changed() {
    let home = home();
    make(home.path(), "vpn/work.conf", 0);
    make(home.path(), "vpn/", 60);
    let mut completer = completer(&home, &[]);

    titles(&mut completer, &file(&[]), "~/vpn/w");
    titles(&mut completer, &file(&[]), "~/vpn/wo");
    assert_eq!(completer.reads(), 1);

    make(home.path(), "vpn/home.conf", 0);
    make(home.path(), "vpn/", 30);
    assert_eq!(titles(&mut completer, &file(&[]), "~/vpn/h"), ["home.conf"]);
    assert_eq!(completer.reads(), 2);
}

#[test]
fn a_directory_changed_within_its_stamps_resolution_is_read_again() {
    let home = home();
    make(home.path(), "vpn/work.conf", 0);
    let mut completer = completer(&home, &[]);

    titles(&mut completer, &file(&[]), "~/vpn/w");
    titles(&mut completer, &file(&[]), "~/vpn/wo");

    assert_eq!(completer.reads(), 2);
}

#[test]
fn the_directory_listed_longest_ago_is_dropped_first() {
    let home = home();
    for n in 0..=CACHED_DIRS {
        make(home.path(), &format!("d{n}/"), 60);
    }
    let mut completer = completer(&home, &[]);

    for n in 0..=CACHED_DIRS {
        titles(&mut completer, &file(&[]), &format!("~/d{n}/"));
    }
    titles(&mut completer, &file(&[]), &format!("~/d{CACHED_DIRS}/"));
    assert_eq!(completer.reads(), CACHED_DIRS as u64 + 1);

    titles(&mut completer, &file(&[]), "~/d0/");
    assert_eq!(completer.reads(), CACHED_DIRS as u64 + 2);
}

#[test]
fn of_several_requests_for_one_field_only_the_last_is_answered() {
    let home = home();
    make(home.path(), "vpn/work.conf", 0);
    let ask = |field: &str, text: &str| Ask {
        front_end: FrontEndId(1),
        extension: "wireguard".into(),
        view: "import".into(),
        field: field.into(),
        text: text.into(),
        wanted: file(&[]),
    };
    let (jobs, queued) = mpsc::channel();
    let (events, answers) = mpsc::channel();
    for (field, text) in [("config", "~/v"), ("into", "~/"), ("config", "~/vpn/")] {
        jobs.send(Job::Ask(ask(field, text))).unwrap();
    }
    drop(jobs);

    serve(completer(&home, &[]), &queued, &events);

    let answered: Vec<(String, String)> = answers
        .try_iter()
        .map(|event| match event {
            Event::Completed {
                message: ToFrontEnd::PathCompletions { field, text, .. },
                ..
            } => (field, text),
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(
        answered,
        [
            ("into".to_owned(), "~/".to_owned()),
            ("config".to_owned(), "~/vpn/".to_owned())
        ]
    );
}

#[test]
fn a_recent_dir_starting_with_a_tilde_is_in_the_home_directory() {
    let home = home();
    make(home.path(), "Inbox/new.conf", 10);
    let mut completer = Completer::new(
        Some(home.path().to_path_buf()),
        Settings {
            recent_dirs: Some(vec![PathBuf::from("~/Inbox"), PathBuf::from("~/Inbox")]),
        },
    );

    assert_eq!(titles(&mut completer, &file(&[]), ""), ["new.conf"]);
}

#[test]
fn a_path_into_nothing_completes_with_nothing() {
    let home = home();
    let mut completer = completer(&home, &[]);

    assert!(titles(&mut completer, &file(&[]), "~/nowhere/a").is_empty());
}

#[test]
fn nothing_typed_offers_the_newest_files_it_takes_from_the_recent_dirs() {
    let home = home();
    make(home.path(), "Downloads/old.conf", 300);
    make(home.path(), "Downloads/new.conf", 10);
    make(home.path(), "Downloads/photo.png", 5);
    make(home.path(), "Downloads/vpn/", 1);
    make(home.path(), "Documents/middle.conf", 100);
    let mut completer = completer(&home, &["Downloads", "Documents", "Desktop"]);

    let found = completer.complete(&file(&["conf"]), "");

    let titles: Vec<&str> = found.iter().map(|entry| entry.title.as_str()).collect();
    assert_eq!(titles, ["new.conf", "middle.conf", "old.conf"]);
    assert_eq!(found[1].path, home.path().join("Documents/middle.conf"));
    assert!(found[0].modified > found[1].modified);
}

#[test]
fn the_completion_thread_answers_with_an_event() {
    let home = home();
    make(home.path(), "vpn/work.conf", 0);
    let mut completions = Completions::Here(Box::new(completer(&home, &[])));
    let (events, received) = mpsc::channel();
    completions.start(events);

    let answered_here = completions.ask(Ask {
        front_end: FrontEndId(7),
        extension: "wireguard".into(),
        view: "import".into(),
        field: "config".into(),
        text: "~/vpn/w".into(),
        wanted: file(&["conf"]),
    });

    assert_eq!(answered_here, None);
    let Ok(Event::Completed { front_end, message }) = received.recv_timeout(Duration::from_secs(5)) else {
        panic!("no answer from the completion thread");
    };
    assert_eq!(front_end, FrontEndId(7));
    let ToFrontEnd::PathCompletions {
        field, text, entries, ..
    } = message
    else {
        panic!("{message:?}");
    };
    assert_eq!((field.as_str(), text.as_str()), ("config", "~/vpn/w"));
    assert_eq!(entries[0].path, home.path().join("vpn/work.conf"));
}

#[test]
fn recent_dirs_come_from_the_config() {
    let config = Table::from([(
        "files".to_owned(),
        Value::Table(Table::from([(
            "recent_dirs".to_owned(),
            Value::Array(vec![Value::String("/srv/inbox".into())]),
        )])),
    )]);
    let mut report = Report::silent();

    let settings = Settings::from_config(&config, &mut report);

    assert_eq!(settings.recent_dirs, Some(vec![PathBuf::from("/srv/inbox")]));
    assert!(report.into_problems().is_empty());
}

#[test]
fn a_recent_dir_that_isnt_a_path_is_a_problem() {
    let config = Table::from([(
        "files".to_owned(),
        Value::Table(Table::from([(
            "recent_dirs".to_owned(),
            Value::Array(vec![Value::String("Downloads".into())]),
        )])),
    )]);
    let mut report = Report::silent();

    let settings = Settings::from_config(&config, &mut report);

    assert_eq!(settings.recent_dirs, None);
    assert!(!report.into_problems().is_empty());
}

#[test]
fn the_user_dirs_come_from_user_dirs_dirs_or_their_usual_names() {
    let home = home();
    let config = home.path().join(".config");
    fs::create_dir_all(&config).unwrap();
    fs::write(
        config.join("user-dirs.dirs"),
        "# Written by xdg-user-dirs-update\n\
         XDG_DOWNLOAD_DIR=\"$HOME/Pobrane\"\n\
         XDG_DESKTOP_DIR=\"$HOME/\"\n",
    )
    .unwrap();

    let dirs = user_dirs(&config, home.path());

    assert_eq!(dirs, [home.path().join("Pobrane"), home.path().join("Documents"),]);
}

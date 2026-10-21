
use super::*;

fn query(at: u64, query: &str) -> Entry {
    Entry {
        at,
        used: Used::Query { query: query.into() },
    }
}

#[test]
fn each_entry_is_a_line_of_json() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state/usage.jsonl");
    let mut log = Log::new(path.clone(), MAX_BYTES);
    log.write(&query(10, "fi")).unwrap();
    log.write(&Entry {
        at: 11,
        used: Used::Pick {
            query: "fi".into(),
            extension: "apps".into(),
            id: "firefox".into(),
            action: "open".into(),
            row: Some(1),
        },
    })
    .unwrap();
    log.write(&Entry {
        at: 12,
        used: Used::Pick {
            query: "fi".into(),
            extension: "apps".into(),
            id: "fish".into(),
            action: "open".into(),
            row: None,
        },
    })
    .unwrap();
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        concat!(
            r#"{"at":10,"kind":"query","query":"fi"}"#,
            "\n",
            r#"{"at":11,"kind":"pick","query":"fi","extension":"apps","id":"firefox","action":"open","row":1}"#,
            "\n",
            r#"{"at":12,"kind":"pick","query":"fi","extension":"apps","id":"fish","action":"open","row":null}"#,
            "\n",
        )
    );
}

#[test]
fn a_full_log_is_rotated_and_never_grows_past_its_limit() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("usage.jsonl");
    let line = r#"{"at":10,"kind":"query","query":"fi"}"#.len() as u64 + 1;
    let mut log = Log::new(path.clone(), 2 * line);
    for _ in 0..5 {
        log.write(&query(10, "fi")).unwrap();
    }
    let older = dir.path().join("usage.jsonl.1");
    assert_eq!(fs::metadata(&older).unwrap().len(), 2 * line);
    assert_eq!(fs::metadata(&path).unwrap().len(), line);
}

#[test]
fn a_log_an_earlier_run_filled_is_rotated_at_the_first_entry() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("usage.jsonl");
    let line = r#"{"at":10,"kind":"query","query":"fi"}"#.len() as u64 + 1;
    let mut log = Log::new(path.clone(), 2 * line);
    log.write(&query(10, "fi")).unwrap();
    log.write(&query(10, "fi")).unwrap();

    Log::new(path.clone(), 2 * line).write(&query(10, "fi")).unwrap();
    let older = dir.path().join("usage.jsonl.1");
    assert_eq!(fs::metadata(&older).unwrap().len(), 2 * line);
    assert_eq!(fs::metadata(&path).unwrap().len(), line);
}

#[test]
fn the_log_keeps_what_an_earlier_run_wrote() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("usage.jsonl");
    Log::new(path.clone(), MAX_BYTES).write(&query(10, "fi")).unwrap();
    Log::new(path.clone(), MAX_BYTES).write(&query(11, "fir")).unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap().lines().count(), 2);
}

fn pick(at: u64, extension: &str, id: &str) -> Entry {
    Entry {
        at,
        used: Used::Pick {
            query: "fi".into(),
            extension: extension.into(),
            id: id.into(),
            action: "open".into(),
            row: Some(0),
        },
    }
}

/// A query and a pick of an app in the rotated log, then a pick of a
/// file and of an app in the current one.
fn rotated_log(dir: &Path) -> (Log, PathBuf, PathBuf) {
    let older = dir.join("usage.jsonl.1");
    let mut log = Log::new(older.clone(), MAX_BYTES);
    log.write(&query(10, "fi")).unwrap();
    log.write(&pick(11, "apps", "firefox")).unwrap();
    let path = dir.join("usage.jsonl");
    let mut log = Log::new(path.clone(), MAX_BYTES);
    log.write(&pick(12, "files", "notes")).unwrap();
    log.write(&pick(13, "apps", "fish")).unwrap();
    (log, path, older)
}

#[test]
fn forgetting_everything_removes_the_log_and_the_rotated_one() {
    let dir = tempfile::tempdir().unwrap();
    let (mut log, path, older) = rotated_log(dir.path());
    assert!(older.exists());
    log.forget(None).unwrap();
    assert!(!path.exists() && !older.exists());

    log.write(&query(13, "fir")).unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap().lines().count(), 1);
    log.forget(None).unwrap();
    log.forget(None).unwrap();
}

#[test]
fn forgetting_one_extension_drops_its_picks_from_both_files() {
    let dir = tempfile::tempdir().unwrap();
    let (mut log, path, older) = rotated_log(dir.path());
    log.forget(Some("apps")).unwrap();
    let lines = |path: &Path| {
        fs::read_to_string(path)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap()["at"].clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(lines(&older), [10]);
    assert_eq!(lines(&path), [12]);

    log.write(&pick(14, "apps", "firefox")).unwrap();
    assert_eq!(lines(&path).len(), 2, "the log goes on after it");
}

#[cfg(unix)]
#[test]
fn only_the_user_can_read_the_log() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("usage.jsonl");
    Log::new(path.clone(), MAX_BYTES).write(&query(10, "fi")).unwrap();
    let mode = fs::metadata(&path).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o600);
}

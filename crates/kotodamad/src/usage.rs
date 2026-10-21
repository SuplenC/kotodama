//! The Usage log: the queries the user typed and the Results they picked, to
//! tune ranking from real use. It is off unless the config turns it on
//! (`[history] usage_log = true`), and it never leaves the machine.
//!
//! It is kept in `$XDG_STATE_HOME/kotodama/usage.jsonl`, one JSON object per
//! line, readable only by the user:
//!
//! ```json
//! {"at":1760870400,"kind":"query","query":"fi"}
//! {"at":1760870401,"kind":"pick","query":"fi","extension":"apps","id":"firefox","action":"open","row":1}
//! ```
//!
//! - `at`: seconds since the Unix epoch.
//! - `kind`: `query` when the user typed, `pick` when they ran an Action.
//! - `query`: what was typed, as the Front end sent it. Typing that empties the search box isn't
//!   logged, but a pick made with nothing typed is, with `"query":""`.
//! - `extension` and `id`: the picked Result's Extension and Item id.
//! - `action`: the id of the Action run.
//! - `row`: where the Result was in the list, from 0, or `null` if it wasn't in the list last sent.
//!
//! When the file would grow past [`MAX_BYTES`], it is renamed to
//! `usage.jsonl.1`, replacing the one before, and a new file is started, so
//! the log never takes more than twice that. Turning the log off keeps what
//! was written; delete the files to forget it. Clearing the History deletes
//! both files, and clearing one Extension's History drops the picks of its
//! Results from them. Undoing either puts back the History, never the log.

use std::fs;
use std::fs::File;
use std::fs::OpenOptions;
use std::io;
use std::io::BufRead;
use std::io::BufReader;
use std::io::BufWriter;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::sync::mpsc::Sender;

use serde::Deserialize;
use serde::Serialize;

use crate::host::spawn;

/// How large `usage.jsonl` grows before it is rotated: 4 MiB, tens of
/// thousands of queries.
pub const MAX_BYTES: u64 = 4 * 1024 * 1024;

/// One line of the log.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Entry {
    /// Seconds since the Unix epoch.
    pub at: u64,
    #[serde(flatten)]
    pub used: Used,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Used {
    /// The user typed `query`.
    Query { query: String },
    /// The user ran `action` on a Result while `query` was typed.
    Pick {
        query: String,
        extension: String,
        id: String,
        action: String,
        /// Its place in the list, from 0, if it was in the list last sent.
        row: Option<usize>,
    },
}

/// Appends entries to the log file, rotating it past its limit.
struct Log {
    path: PathBuf,
    max: u64,
    /// The file, and how large it is; opened at the first entry.
    file: Option<(File, u64)>,
    line: Vec<u8>,
}

impl Log {
    fn new(path: PathBuf, max: u64) -> Self {
        Log {
            path,
            max,
            file: None,
            line: Vec::new(),
        }
    }

    fn write(&mut self, entry: &Entry) -> io::Result<()> {
        self.line.clear();
        serde_json::to_writer(&mut self.line, entry)?;
        self.line.push(b'\n');
        let len = self.line.len() as u64;
        let (mut file, mut size) = match self.file.take() {
            Some(open) => open,
            None => {
                let file = open(&self.path)?;
                let size = file.metadata()?.len();
                (file, size)
            }
        };
        // A file an earlier run left full is rotated too.
        if size > 0 && size + len > self.max {
            fs::rename(&self.path, rotated(&self.path))?;
            (file, size) = (open(&self.path)?, 0);
        }
        file.write_all(&self.line)?;
        size += len;
        self.file = Some((file, size));
        Ok(())
    }

    /// Forgets the log, current and rotated: all of it, or only the picks of
    /// one Extension's Results.
    fn forget(&mut self, extension: Option<&str>) -> io::Result<()> {
        self.file = None;
        for path in [rotated(&self.path), self.path.clone()] {
            let kept = match extension {
                None => fs::remove_file(&path),
                Some(extension) => drop_picks(&path, extension),
            };
            match kept {
                Err(err) if err.kind() != io::ErrorKind::NotFound => return Err(err),
                _ => {}
            }
        }
        Ok(())
    }
}

/// The part of a line that says whose Result was picked.
#[derive(Deserialize)]
struct Picked {
    extension: Option<String>,
}

/// Rewrites the log at `path` without the picks of `extension`'s Results,
/// replacing it whole so a crash leaves the old one.
fn drop_picks(path: &Path, extension: &str) -> io::Result<()> {
    let lines = BufReader::new(File::open(path)?).split(b'\n');
    let mut partial = path.as_os_str().to_owned();
    partial.push(".partial");
    let partial = PathBuf::from(partial);
    let _ = fs::remove_file(&partial);
    let mut kept = BufWriter::new(open(&partial)?);
    let mut dropped = false;
    for line in lines {
        let line = line?;
        let picked =
            serde_json::from_slice::<Picked>(&line).is_ok_and(|picked| picked.extension.as_deref() == Some(extension));
        if picked {
            dropped = true;
        } else {
            kept.write_all(&line)?;
            kept.write_all(b"\n")?;
        }
    }
    kept.flush()?;
    drop(kept);
    if dropped {
        fs::rename(&partial, path)
    } else {
        fs::remove_file(&partial)
    }
}

/// The older log, which the current one replaces when it's full.
fn rotated(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".1");
    PathBuf::from(name)
}

/// Opens the log to append, readable by the user alone.
fn open(path: &Path) -> io::Result<File> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options.open(path)
}

/// How many entries may wait for a slow disk; past this, new ones are dropped
/// rather than held in memory.
const PENDING: usize = 1024;

/// Forgets the log at `path`, current and rotated, on a thread of its own, as
/// [`Writer::forget`] does, for when no Writer is running.
pub fn forget(path: PathBuf, extension: Option<String>) {
    spawn("usage".into(), move || {
        if let Err(err) = Log::new(path, MAX_BYTES).forget(extension.as_deref()) {
            log::error!("couldn't forget the usage log: {err}");
        }
    });
}

/// What the Writer's thread is asked to do.
enum Op {
    Write(Entry),
    /// Forget the log, or one Extension's picks in it.
    Forget(Option<String>),
}

/// Writes the log on a thread of its own, so the Daemon never waits for the
/// disk. The thread ends when this is dropped.
pub struct Writer {
    ops: Sender<Op>,
    /// How many entries wait to be written.
    pending: Arc<AtomicUsize>,
}

impl Writer {
    pub fn new(path: PathBuf) -> Self {
        let (ops, queue) = mpsc::channel::<Op>();
        let pending = Arc::new(AtomicUsize::new(0));
        let waiting = Arc::clone(&pending);
        spawn("usage".into(), move || {
            let mut log = Log::new(path, MAX_BYTES);
            let mut failing = false;
            while let Ok(op) = queue.recv() {
                match op {
                    Op::Write(entry) => {
                        waiting.fetch_sub(1, Ordering::Relaxed);
                        match log.write(&entry) {
                            Ok(()) => failing = false,
                            Err(err) if !failing => {
                                failing = true;
                                log::error!("couldn't write the usage log: {err}");
                            }
                            Err(_) => {}
                        }
                    }
                    Op::Forget(extension) => {
                        if let Err(err) = log.forget(extension.as_deref()) {
                            log::error!("couldn't forget the usage log: {err}");
                        }
                    }
                }
            }
        });
        Writer { ops, pending }
    }

    /// Never blocks: an entry that finds [`PENDING`] others waiting is dropped.
    pub fn write(&self, entry: Entry) {
        if self.pending.load(Ordering::Relaxed) < PENDING {
            self.pending.fetch_add(1, Ordering::Relaxed);
            let _ = self.ops.send(Op::Write(entry));
        }
    }

    /// Forgets the log, current and rotated, once the entries before are
    /// written: all of it, or only the picks of `extension`'s Results. Never
    /// blocks, and is never dropped for a full queue.
    pub fn forget(&self, extension: Option<String>) {
        let _ = self.ops.send(Op::Forget(extension));
    }
}

#[cfg(test)]
mod tests {
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
}

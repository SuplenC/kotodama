//! The usage log: the queries the user typed and the Results they picked, to
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
//! - `query`: what was typed, as the Front end sent it. An empty query isn't
//!   logged.
//! - `extension` and `id`: the picked Result's Extension and Item id.
//! - `action`: the id of the Action run.
//! - `row`: where the Result was in the list, from 0, or `null` if it wasn't
//!   in the list last sent.
//!
//! When the file would grow past [`MAX_BYTES`], it is renamed to
//! `usage.jsonl.1`, replacing the one before, and a new file is started, so
//! the log never takes more than twice that. Turning the log off keeps what
//! was written; delete the files to forget it.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};

use serde::Serialize;

use crate::host::spawn;

/// How large `usage.jsonl` grows before it is rotated: 4 MiB, a few months
/// of typical use.
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
        // A file an earlier run left fuller than this is rotated too.
        if self
            .file
            .as_ref()
            .is_some_and(|(_, size)| *size > 0 && size + len > self.max)
        {
            self.file = None;
            fs::rename(&self.path, rotated(&self.path))?;
        }
        let (file, size) = match &mut self.file {
            Some(open) => open,
            None => {
                let file = open(&self.path)?;
                let size = file.metadata()?.len();
                self.file.insert((file, size))
            }
        };
        file.write_all(&self.line)?;
        *size += len;
        Ok(())
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

/// Writes the log on a thread of its own, so the Daemon never waits for the
/// disk. The thread starts at the first entry and ends when this is dropped.
pub struct Writer {
    entries: Sender<Entry>,
}

impl Writer {
    pub fn new(path: PathBuf) -> Self {
        let (entries, pending) = mpsc::channel::<Entry>();
        spawn("usage".into(), move || {
            let mut log = Log::new(path, MAX_BYTES);
            let mut failing = false;
            while let Ok(entry) = pending.recv() {
                match log.write(&entry) {
                    Ok(()) => failing = false,
                    Err(err) if !failing => {
                        failing = true;
                        log::error!("couldn't write the usage log: {err}");
                    }
                    Err(_) => {}
                }
            }
        });
        Writer { entries }
    }

    /// Never blocks.
    pub fn write(&self, entry: Entry) {
        let _ = self.entries.send(entry);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query(at: u64, query: &str) -> Entry {
        Entry {
            at,
            used: Used::Query {
                query: query.into(),
            },
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
    fn the_log_keeps_what_an_earlier_run_wrote() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("usage.jsonl");
        Log::new(path.clone(), MAX_BYTES)
            .write(&query(10, "fi"))
            .unwrap();
        Log::new(path.clone(), MAX_BYTES)
            .write(&query(11, "fir"))
            .unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap().lines().count(), 2);
    }

    #[cfg(unix)]
    #[test]
    fn only_the_user_can_read_the_log() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("usage.jsonl");
        Log::new(path.clone(), MAX_BYTES)
            .write(&query(10, "fi"))
            .unwrap();
        let mode = fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}

//! A Front end on the real Daemon, run in this process, with the real emoji
//! Extension as its only Extension. Shared by the search tests and the
//! benchmark, each of which includes it with `#[path]`: a file under `tests/` would be a test
//! target of its own, and a `mod.rs` is not used.

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::net::UnixStream;
use std::thread;
use std::time::Duration;
use std::time::Instant;

use kotodama_extension::testing;
use kotodama_protocol::ActionRef;
use kotodama_protocol::Channel;
use kotodama_protocol::FromFrontEnd;
use kotodama_protocol::ResultRef;
use kotodama_protocol::ShownResult;
use kotodama_protocol::ToFrontEnd;
use kotodamad::paths::Paths;
use tempfile::TempDir;

pub struct Menu {
    _dir: TempDir,
    ui: Channel<UnixStream, UnixStream>,
    /// The version of the last Results read.
    shown: u32,
}

impl Menu {
    /// Starts the Daemon and waits until the emoji Extension's Items are
    /// indexed. `config` is written as the Daemon's config file.
    pub fn start(config: &str) -> Self {
        Self::start_with(config, "emoji thumbs")
    }

    /// As [`Menu::start`], knowing the emoji are indexed once `ready` finds
    /// any: a config that puts them in the search needs another query. The
    /// menu opens in the emoji app, as `kotodama open --app emoji` does,
    /// so a query that starts with `emoji ` searches them alone.
    pub fn start_with(config: &str, ready: &str) -> Self {
        let dir = tempfile::tempdir().expect("make the Daemon's dir");
        let path = |sub: &str| dir.path().join(sub);
        fs::create_dir_all(path("config")).expect("make the config dir");
        fs::write(path("config/config.toml"), config).expect("write the config");
        // The Bundled extension's own Manifest, run from where cargo built it.
        testing::install(
            &path("extensions"),
            "emoji",
            &testing::bundled(
                include_str!("../../manifest.toml"),
                env!("CARGO_BIN_EXE_kotodama-emoji"),
            ),
        );

        let paths = Paths {
            socket: path("kotodama.sock"),
            config_dirs: vec![path("config")],
            extension_dirs: vec![path("extensions")],
            history: path("state/history.json"),
            usage_log: path("state/usage.jsonl"),
            cache: path("cache"),
        };
        let socket = paths.socket.clone();
        thread::spawn(move || kotodamad::run(&paths));

        let ui = testing::connect(&socket, Duration::from_secs(1));
        let mut menu = Menu {
            _dir: dir,
            ui,
            shown: 0,
        };
        menu.ui
            .send(&FromFrontEnd::Open {
                query: "emoji ".into(),
                view: None,
            })
            .expect("open the emoji app");
        // The Results change once the Items are indexed; from then on each
        // query has one answer.
        let started = Instant::now();
        let mut results = menu.search(ready);
        while results.is_empty() {
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "the emoji never got indexed"
            );
            results = menu.results();
        }
        menu
    }

    /// Sends `query` and returns its Results.
    pub fn search(&mut self, query: &str) -> Vec<ShownResult> {
        self.ui
            .send(&FromFrontEnd::Query { text: query.into() })
            .expect("send the query");
        self.results()
    }

    /// The next Results the Daemon sends.
    fn results(&mut self) -> Vec<ShownResult> {
        loop {
            match self.ui.recv::<ToFrontEnd>() {
                Ok(Some(ToFrontEnd::Results { version, results, .. })) => {
                    self.shown = version;
                    return results;
                }
                Ok(Some(_)) => {}
                other => panic!("expected Results, got {other:?}"),
            }
        }
    }

    /// The ids of `query`'s Results, best first.
    pub fn ids(&mut self, query: &str) -> Vec<String> {
        self.search(query).into_iter().map(|result| result.item.id).collect()
    }

    /// Runs the primary Action of the Result `id`, which must close the menu,
    /// and says the menu is closed, as a Front end does.
    pub fn pick(&mut self, id: &str) {
        self.ui
            .send(&FromFrontEnd::RunAction {
                result: ResultRef {
                    extension: "emoji".into(),
                    id: id.into(),
                },
                shown: self.shown,
                action: ActionRef {
                    extension: "emoji".into(),
                    id: "type".into(),
                },
                arguments: BTreeMap::default(),
            })
            .expect("run the Action");
        loop {
            match self.ui.recv::<ToFrontEnd>() {
                Ok(Some(ToFrontEnd::Close)) => break,
                Ok(Some(_)) => {}
                other => panic!("expected Close, got {other:?}"),
            }
        }
        self.ui.send(&FromFrontEnd::Closed).expect("say the menu closed");
    }
}

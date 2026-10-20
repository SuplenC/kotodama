//! Drives the clipboard Extension the way the Daemon does: over its stdio,
//! with its state directory in a temporary directory. There is no Wayland
//! display here, so the history is filled through the library, as copies
//! would fill it; `e2e/clipboard` watches a real clipboard.

use std::collections::BTreeMap;
use std::fs;
use std::io::Cursor;
use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use kotodama_clipboard::copied::Copied;
use kotodama_clipboard::history::{History, Limits};
use kotodama_extension::testing::Spawned;
use kotodama_protocol::*;
use tempfile::TempDir;

const WAIT: Duration = Duration::from_secs(5);
const DAY: u64 = 24 * 60 * 60;

struct Extension(Spawned);

impl Deref for Extension {
    type Target = Spawned;
    fn deref(&self) -> &Spawned {
        &self.0
    }
}

impl DerefMut for Extension {
    fn deref_mut(&mut self) -> &mut Spawned {
        &mut self.0
    }
}

impl Extension {
    /// Starts the Extension on the state in `state`, without its config.
    fn unconfigured(state: &Path) -> Self {
        Extension(Spawned::start(
            Command::new(env!("CARGO_BIN_EXE_kotodama-clipboard"))
                .env("XDG_STATE_HOME", state)
                .env_remove("WAYLAND_DISPLAY"),
            WAIT,
        ))
    }

    /// Starts the Extension on the state in `state`, and sends it `config`.
    fn start(state: &Path, config: Table) -> Self {
        let mut extension = Extension::unconfigured(state);
        extension.send(ToExtension::Config(config));
        extension
    }

    fn pushed(&self) -> View {
        match self.recv() {
            FromExtension::PushView(view) => view,
            other => panic!("expected push_view, got {other:?}"),
        }
    }

    /// Runs `action` on the Item `id`, of `kind`, without waiting for the
    /// outcome.
    fn start_run(&mut self, id: &str, kind: &str, action: &str) {
        let item = ItemRef {
            id: id.into(),
            kind: ItemKind::Namespaced(NamespacedKind::new(kind).unwrap()),
        };
        self.0.start_run(7, action, item, BTreeMap::new());
    }

    /// The outcome of the Action run last.
    fn outcome(&self) -> Outcome {
        self.0.outcome(7, |_| false)
    }

    /// Runs `action` on the clip `id` and returns its outcome.
    fn run(&mut self, id: &str, action: &str) -> Outcome {
        self.start_run(id, "clipboard.clip", action);
        self.outcome()
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

/// The history directory under a state directory.
fn history_dir(state: &Path) -> PathBuf {
    state.join("kotodama/clipboard")
}

/// Records what was copied, `ago` seconds ago, as watching the clipboard
/// would. Returns the clip's Item id.
fn copy(state: &Path, copied: Copied, ago: u64) -> String {
    let mut history = History::open(&history_dir(state));
    let recorded = history
        .record(copied, now() - ago, &Limits::default())
        .unwrap();
    recorded.id.to_string()
}

fn text(text: &str) -> Copied {
    Copied::read("text/plain;charset=utf-8", text.as_bytes().to_vec()).unwrap()
}

fn png(width: u32, height: u32) -> Vec<u8> {
    let image = image::RgbImage::from_pixel(width, height, image::Rgb([20, 90, 200]));
    let mut bytes = Cursor::new(Vec::new());
    image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
    bytes.into_inner()
}

fn titles(items: &[Item]) -> Vec<&str> {
    items.iter().map(|item| item.title.as_str()).collect()
}

fn clips(items: &[Item]) -> Vec<&Item> {
    items
        .iter()
        .filter(|item| item.kind.name() == "clipboard.clip")
        .collect()
}

#[test]
fn the_history_is_sent_as_items_after_a_restart() {
    let state = tempfile::tempdir().unwrap();
    copy(state.path(), text("cargo nextest run"), 60);
    copy(state.path(), text("git push\n--force-with-lease"), 30);
    let extension = Extension::start(state.path(), Table::new());

    let items = extension.added();
    let history = items.iter().find(|item| item.id == "history").unwrap();
    assert_eq!(history.title, "Clipboard History");
    assert_eq!(
        history
            .actions
            .iter()
            .map(|action| action.title.as_str())
            .collect::<Vec<_>>(),
        ["Show Clipboard History", "Clear Clipboard History"]
    );

    let clips = clips(&items);
    assert_eq!(
        titles(&clips.iter().copied().cloned().collect::<Vec<_>>()),
        ["git push --force-with-lease", "cargo nextest run"]
    );
    let clip = clips[0];
    assert_eq!(clip.subtitle.as_deref(), Some("Text, 27 characters"));
    assert_eq!(
        clip.actions
            .iter()
            .map(|action| action.id.as_str())
            .collect::<Vec<_>>(),
        ["paste", "copy", "show", "delete"]
    );
}

#[test]
fn the_primary_action_pastes_the_whole_clip() {
    let state = tempfile::tempdir().unwrap();
    let long = format!("first line\n{}", "y".repeat(300));
    let id = copy(state.path(), text(&long), 10);
    let data = png(64, 32);
    let image = copy(
        state.path(),
        Copied::read("image/png", data.clone()).unwrap(),
        5,
    );
    let mut extension = Extension::start(state.path(), Table::new());
    extension.added();

    assert_eq!(
        extension.run(&id, "paste"),
        Outcome::Paste(Clipboard::Text(long))
    );
    assert_eq!(
        extension.run(&image, "paste"),
        Outcome::Paste(Clipboard::Image {
            mime: "image/png".into(),
            data: Bytes(data),
        })
    );
}

#[test]
fn an_image_shows_its_thumbnail_in_the_detail_view() {
    let state = tempfile::tempdir().unwrap();
    let id = copy(
        state.path(),
        Copied::read("image/png", png(800, 400)).unwrap(),
        90,
    );
    let mut extension = Extension::start(state.path(), Table::new());
    let items = extension.added();
    let clip = clips(&items)[0];
    assert_eq!(clip.title, "Image, 800×400");
    // The thumbnail is the Item's icon too.
    let thumbnail = PathBuf::from(clip.icon.clone().unwrap());

    extension.start_run(&id, "clipboard.clip", "show");
    let view = extension.pushed();
    assert_eq!(extension.outcome(), Outcome::KeepOpen);
    let ViewContent::Detail { markdown, metadata } = view.content else {
        panic!("expected a detail View, got {view:?}");
    };
    assert_eq!(
        markdown,
        format!("![Image, 800×400](file://{})", thumbnail.display())
    );
    let shown = image::open(&thumbnail).unwrap();
    assert_eq!((shown.width(), shown.height()), (256, 128));
    let metadata: Vec<(&str, &str)> = metadata
        .iter()
        .map(|entry| (entry.label.as_str(), entry.value.as_str()))
        .collect();
    assert_eq!(metadata[0], ("Copied", "1 minute ago"));
    assert_eq!(metadata[2], ("Type", "image/png"));
}

#[test]
fn a_text_shows_whole_in_the_detail_view() {
    let state = tempfile::tempdir().unwrap();
    let id = copy(state.path(), text("let x = `y`;\nlet z = 1;"), 0);
    let mut extension = Extension::start(state.path(), Table::new());
    extension.added();

    extension.start_run(&id, "clipboard.clip", "show");
    let view = extension.pushed();
    let ViewContent::Detail { markdown, .. } = view.content else {
        panic!("expected a detail View, got {view:?}");
    };
    assert_eq!(markdown, "```\nlet x = `y`;\nlet z = 1;\n```\n");
}

#[test]
fn deleting_an_entry_removes_it_for_good() {
    let state = tempfile::tempdir().unwrap();
    let keep = copy(state.path(), text("keep me"), 20);
    let gone = copy(state.path(), text("forget me"), 10);
    let mut extension = Extension::start(state.path(), Table::new());
    extension.added();

    extension.start_run(&gone, "clipboard.clip", "delete");
    assert_eq!(extension.removed(), [gone]);
    assert_eq!(extension.outcome(), Outcome::KeepOpen);
    drop(extension);

    let extension = Extension::start(state.path(), Table::new());
    let items = extension.added();
    let ids: Vec<&str> = clips(&items).iter().map(|item| item.id.as_str()).collect();
    assert_eq!(ids, [keep.as_str()]);
}

#[test]
fn the_clear_action_empties_the_history_on_disk() {
    let state = tempfile::tempdir().unwrap();
    let first = copy(state.path(), text("one"), 20);
    let second = copy(
        state.path(),
        Copied::read("image/png", png(10, 10)).unwrap(),
        10,
    );
    let mut extension = Extension::start(state.path(), Table::new());
    extension.added();

    extension.start_run("history", "clipboard.history", "clear");
    let mut removed = extension.removed();
    removed.sort();
    assert_eq!(removed, [first, second]);
    assert_eq!(extension.outcome(), Outcome::KeepOpen);

    let dir = history_dir(state.path());
    let left: Vec<String> = fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(left, ["index.json"]);
    assert!(History::open(&dir).is_empty());
}

#[test]
fn the_history_item_lists_every_entry_newest_first() {
    let state = tempfile::tempdir().unwrap();
    copy(state.path(), text("older"), 20);
    let newer = copy(state.path(), text("newer"), 10);
    let mut extension = Extension::start(state.path(), Table::new());
    extension.added();

    extension.start_run("history", "clipboard.history", "open");
    let view = extension.pushed();
    assert_eq!(extension.outcome(), Outcome::KeepOpen);
    let ViewContent::List { items, .. } = view.content else {
        panic!("expected a list View, got {view:?}");
    };
    assert_eq!(titles(&items), ["newer", "older"]);

    // Deleting from the list updates it.
    extension.start_run(&newer, "clipboard.clip", "delete");
    assert_eq!(extension.removed(), [newer]);
    match extension.recv() {
        FromExtension::UpdateView(View {
            content: ViewContent::List { items, .. },
            ..
        }) => assert_eq!(titles(&items), ["older"]),
        other => panic!("expected update_view, got {other:?}"),
    }
    assert_eq!(extension.outcome(), Outcome::KeepOpen);
}

#[test]
fn entries_past_the_configured_limits_expire_on_start() {
    let state = tempfile::tempdir().unwrap();
    copy(state.path(), text("eight days old"), 8 * DAY);
    copy(state.path(), text("two days old"), 2 * DAY);
    copy(state.path(), text("an hour old"), 3600);
    copy(state.path(), text("just now"), 0);
    let mut config = Table::new();
    config.insert("max_entries".into(), Value::Integer(2));
    let extension = Extension::start(state.path(), config);

    let items = extension.added();
    assert_eq!(
        titles(&clips(&items).into_iter().cloned().collect::<Vec<_>>()),
        ["just now", "an hour old"]
    );
    assert_eq!(History::open(&history_dir(state.path())).len(), 2);
}

#[test]
fn a_longer_age_in_the_config_keeps_what_the_default_would_expire() {
    let state = tempfile::tempdir().unwrap();
    copy(state.path(), text("eight days old"), 8 * DAY);
    let mut config = Table::new();
    config.insert("max_age_days".into(), Value::Integer(30));
    let mut extension = Extension::unconfigured(state.path());
    // A while without its config, which it waits for before expiring
    // anything by limits it doesn't know yet.
    thread::sleep(Duration::from_millis(300));
    extension.send(ToExtension::Config(config));
    match extension.recv() {
        FromExtension::AddItems { items } => {
            assert_eq!(
                titles(&clips(&items).into_iter().cloned().collect::<Vec<_>>()),
                ["eight days old"]
            )
        }
        other => panic!("expected add_items, got {other:?}"),
    }
}

#[test]
fn a_shorter_age_in_a_new_config_expires_entries_at_once() {
    let state = tempfile::tempdir().unwrap();
    let old = copy(state.path(), text("two days old"), 2 * DAY);
    copy(state.path(), text("just now"), 0);
    let mut extension = Extension::start(state.path(), Table::new());
    assert_eq!(clips(&extension.added()).len(), 2);

    let mut config = Table::new();
    config.insert("max_age_days".into(), Value::Integer(1));
    extension.send(ToExtension::Config(config));
    assert_eq!(extension.removed(), [old]);
}

#[test]
fn copying_without_a_clipboard_says_why() {
    let state = tempfile::tempdir().unwrap();
    let id = copy(state.path(), text("hello"), 0);
    let mut extension = Extension::start(state.path(), Table::new());
    extension.added();

    match extension.run(&id, "copy") {
        Outcome::Failed { message } => assert!(message.contains("clipboard"), "{message}"),
        other => panic!("expected a failure, got {other:?}"),
    }
}

#[test]
fn an_entry_gone_since_it_was_shown_fails_gently() {
    let state: TempDir = tempfile::tempdir().unwrap();
    let mut extension = Extension::start(state.path(), Table::new());
    assert!(clips(&extension.added()).is_empty());
    match extension.run("42", "paste") {
        Outcome::Failed { message } => assert!(message.contains("no longer"), "{message}"),
        other => panic!("expected a failure, got {other:?}"),
    }
}

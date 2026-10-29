//! A keystroke in a file field: completing a partial path in a directory of
//! 1,000 entries, as the `completion` thread does once the directory is
//! listed (one `stat` of the directory, then matching and ranking), and the
//! first keystroke in it, which reads the directory.

use std::fs;
use std::fs::File;
use std::hint::black_box;
use std::path::Path;
use std::time::Duration;
use std::time::SystemTime;

use criterion::Criterion;
use criterion::criterion_group;
use criterion::criterion_main;
use kotodamad::completion::Completer;
use kotodamad::completion::Field;
use kotodamad::completion::Settings;
use tempfile::TempDir;

/// Entries in the directory completed in.
const ENTRIES: usize = 1000;

/// A home directory with `files/` holding [`ENTRIES`] files and folders of
/// made-up names, in the build's target directory rather than `/tmp`.
fn home() -> TempDir {
    let scratch = Path::new(env!("CARGO_TARGET_TMPDIR"));
    let home = tempfile::Builder::new()
        .prefix("completion-")
        .tempdir_in(scratch)
        .unwrap();
    let files = home.path().join("files");
    fs::create_dir_all(&files).unwrap();
    let words = [
        "work", "notes", "report", "photo", "invoice", "config", "backup", "draft",
    ];
    let extensions = ["conf", "txt", "pdf", "png"];
    for n in 0..ENTRIES {
        let name = format!("{}-{n:04}", words[n % words.len()]);
        if n % 10 == 0 {
            fs::create_dir(files.join(name)).unwrap();
        } else {
            fs::write(files.join(format!("{name}.{}", extensions[n % extensions.len()])), "").unwrap();
        }
    }
    // Older than the coarsest modified times a file system keeps, so the
    // listing is trusted from one keystroke to the next.
    let long_ago = SystemTime::now() - Duration::from_secs(60);
    File::open(&files).unwrap().set_modified(long_ago).unwrap();
    home
}

fn completion(c: &mut Criterion) {
    let home = home();
    let conf = Field {
        accept: vec!["conf".into()],
        folder: false,
    };
    let none = Settings {
        recent_dirs: Some(Vec::new()),
    };
    let mut completer = Completer::new(Some(home.path().to_path_buf()), none.clone());
    let mut group = c.benchmark_group("completion_1k_entries");

    completer.complete(&conf, "~/files/w");
    group.bench_function("keystroke", |b| {
        b.iter(|| black_box(completer.complete(&conf, black_box("~/files/wor"))));
    });
    group.bench_function("listing_all", |b| {
        b.iter(|| black_box(completer.complete(&conf, black_box("~/files/"))));
    });
    group.bench_function("first_keystroke_reads_the_directory", |b| {
        b.iter(|| {
            let mut cold = Completer::new(Some(home.path().to_path_buf()), none.clone());
            black_box(cold.complete(&conf, black_box("~/files/w")))
        });
    });
    group.finish();
}

criterion_group!(benches, completion);
criterion_main!(benches);

use std::fs;
use std::path::Path;

use super::Accept;
use super::Globs;

/// A shared MIME-info database of its own, in the build's target directory,
/// which the test leaves there: it is the same every run.
fn globs(name: &str) -> Globs {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/scratch")
        .join(format!("mime-{name}"));
    fs::create_dir_all(dir.join("mime")).unwrap();
    fs::write(
        dir.join("mime/globs2"),
        "# A comment\n\
         50:application/pdf:*.pdf\n\
         50:image/png:*.png\n\
         50:image/jpeg:*.jpg\n\
         50:image/jpeg:*.JPEG\n\
         10:text/x-makefile:Makefile\n\
         50:application/x-tar:*.tar.[gz]\n",
    )
    .unwrap();
    Globs::in_dirs(vec![dir])
}

fn accept(wanted: &[&str], globs: &mut Globs) -> Accept {
    let wanted: Vec<String> = wanted.iter().map(|&wanted| wanted.to_owned()).collect();
    Accept::new(&wanted, globs)
}

#[test]
fn no_accept_takes_any_file() {
    let any = accept(&[], &mut globs("any"));

    assert!(any.takes("notes"));
    assert!(any.takes("work.conf"));
}

#[test]
fn an_extension_takes_files_ending_in_it_whatever_their_case() {
    let conf = accept(&["conf", ".ovpn", "tar.gz"], &mut globs("extensions"));

    assert!(conf.takes("work.conf"));
    assert!(conf.takes("WORK.CONF"));
    assert!(conf.takes("home.ovpn"));
    assert!(conf.takes("backup.tar.gz"));
    assert!(!conf.takes("work.conf.bak"));
    assert!(!conf.takes("conf"));
    assert!(!conf.takes(".conf"));
}

#[test]
fn a_mime_type_takes_the_extensions_the_database_knows_it_by() {
    let mut globs = globs("mime");
    let pdf = accept(&["application/pdf"], &mut globs);
    let images = accept(&["image/*"], &mut globs);

    assert!(pdf.takes("paper.pdf"));
    assert!(!pdf.takes("paper.png"));
    assert!(images.takes("cat.png"));
    assert!(images.takes("cat.jpeg"));
    assert!(images.takes("cat.jpg"));
    assert!(!images.takes("paper.pdf"));
}

#[test]
fn a_mime_type_nothing_is_known_by_takes_nothing() {
    let unknown = accept(&["application/x-nothing"], &mut globs("unknown"));

    assert!(!unknown.takes("work.conf"));
}

use super::*;

const NOW: u64 = 1_800_000_000;

#[test]
fn nothing_copied_is_kept_until_the_pause_ends() {
    let pause = Pause::Until(NOW + 300);

    assert!(!pause.keeps(NOW));
    assert!(!pause.keeps(NOW + 299));
    assert!(pause.keeps(NOW + 300));
    assert!(!Pause::UntilResumed.keeps(NOW + 1_000_000));
    assert!(Pause::Off.keeps(NOW));
}

#[test]
fn the_note_counts_the_minutes_left_and_goes_with_the_pause() {
    let pause = Pause::Until(NOW + 3600);

    assert_eq!(
        pause.note(NOW).as_deref(),
        Some("Paused for 60 more minutes: nothing copied is kept")
    );
    assert_eq!(
        pause.note(NOW + 3599).as_deref(),
        Some("Paused for 1 more minute: nothing copied is kept")
    );
    assert_eq!(pause.note(NOW + 3600), None);
    assert_eq!(
        Pause::UntilResumed.note(NOW).as_deref(),
        Some("Paused until you resume it: nothing copied is kept")
    );
    assert_eq!(Pause::Off.note(NOW), None);
}

#[test]
fn the_next_change_is_when_the_minutes_left_drop_or_the_pause_ends() {
    let pause = Pause::Until(NOW + 300);

    assert_eq!(pause.next_change(NOW), Some(60));
    assert_eq!(pause.next_change(NOW + 30), Some(30));
    assert_eq!(pause.next_change(NOW + 250), Some(50));
    assert_eq!(pause.next_change(NOW + 300), None);
    assert_eq!(Pause::UntilResumed.next_change(NOW), None);
    assert_eq!(Pause::Off.next_change(NOW), None);
}

#[test]
fn a_pause_outlasts_a_restart_until_it_ends() {
    let dir = tempfile::tempdir().unwrap();

    Pause::Until(NOW + 60).save(dir.path()).unwrap();
    let timed = (Pause::load(dir.path(), NOW), Pause::load(dir.path(), NOW + 60));
    Pause::UntilResumed.save(dir.path()).unwrap();
    let until_resumed = Pause::load(dir.path(), NOW + 1_000_000);
    Pause::Off.save(dir.path()).unwrap();
    let resumed = Pause::load(dir.path(), NOW);

    assert_eq!(timed, (Pause::Until(NOW + 60), Pause::Off));
    assert_eq!(until_resumed, Pause::UntilResumed);
    assert_eq!(resumed, Pause::Off);
    assert!(!dir.path().join(FILE).exists());
}

#[test]
fn without_a_saved_pause_copies_are_kept() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join(FILE), "soon").unwrap();

    assert_eq!(Pause::load(&dir.path().join("none"), NOW), Pause::Off);
    assert_eq!(Pause::load(dir.path(), NOW), Pause::Off);
}

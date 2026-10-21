use super::*;

fn os(value: &str) -> OsString {
    value.into()
}

#[test]
fn the_variable_is_used_when_it_names_a_directory() {
    assert_eq!(
        base(Some(os("/x/state")), Some(os("/home/me")), ".local/state"),
        Some(PathBuf::from("/x/state"))
    );
}

#[test]
fn home_is_used_when_the_variable_is_unset_empty_or_relative() {
    for var in [None, Some(os("")), Some(os("relative/state"))] {
        assert_eq!(
            base(var, Some(os("/home/me")), ".local/state"),
            Some(PathBuf::from("/home/me/.local/state"))
        );
    }
}

#[test]
fn neither_set_is_none() {
    assert_eq!(base(None, None, ".cache"), None);
    assert_eq!(base(Some(os("")), Some(os("")), ".cache"), None);
}

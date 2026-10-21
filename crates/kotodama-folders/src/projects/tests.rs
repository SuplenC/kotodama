use super::*;

#[test]
fn gitmodules_paths_are_read_from_submodule_sections_only() {
    let text = "[core]\n\tpath = no\n[submodule \"a\"]\n\tpath = vendor/a\n\turl = x\n\
                [submodule \"b\"]\n  Path = \"b c\"\n";
    assert_eq!(submodule_paths(text), ["vendor/a", "b c"]);
}

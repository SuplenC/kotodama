use super::*;

fn verbs(keywords: &[&str]) -> Verbs {
    Verbs::new(keywords.iter().map(|&keyword| Box::from(keyword)).collect())
}

fn subject(verbs: &Verbs, query: &str) -> Option<String> {
    verbs.find(query).map(|verb| verb.subject)
}

#[test]
fn a_verb_is_found_in_any_place_and_leaves_the_rest_as_the_subject() {
    let verbs = verbs(&["remove", "uninstall"]);

    assert_eq!(
        subject(&verbs, "remove google chrome").as_deref(),
        Some("google chrome")
    );
    assert_eq!(
        subject(&verbs, "google chrome remove").as_deref(),
        Some("google chrome")
    );
    assert_eq!(
        subject(&verbs, "Google UNINSTALL chrome").as_deref(),
        Some("Google chrome")
    );
}

#[test]
fn three_letters_start_a_verb_and_fewer_must_be_all_of_it() {
    let verbs = verbs(&["remove", "rm"]);

    assert_eq!(subject(&verbs, "rem chrome").as_deref(), Some("chrome"));
    assert_eq!(subject(&verbs, "re chrome"), None);
    assert_eq!(subject(&verbs, "rm chrome").as_deref(), Some("chrome"));
    assert_eq!(subject(&verbs, "removed chrome"), None);
}

#[test]
fn a_verb_alone_names_nothing() {
    let verbs = verbs(&["remove"]);

    assert_eq!(subject(&verbs, "remove"), None);
    assert_eq!(subject(&verbs, " remove  "), None);
}

#[test]
fn a_word_names_every_keyword_it_starts() {
    let verbs = verbs(&["delete", "remote", "remove", "rename"]);

    let verb = verbs.find("rem x").unwrap();

    assert!(verb.names_folded("remove"));
    assert!(verb.names_folded("remote"));
    assert!(!verb.names_folded("rename"));
    assert!(!verb.names_folded("delete"));
}

#[test]
fn a_verb_is_folded_as_the_query_is() {
    let verbs = Verbs::new(vec![fold_text("usuń").into()]);

    assert_eq!(subject(&verbs, "USUŃ chrome").as_deref(), Some("chrome"));
    assert!(
        verbs
            .find("usun chrome")
            .unwrap()
            .names(&Action::new("remove", "Remove").keywords(["Usuń"]))
    );
}

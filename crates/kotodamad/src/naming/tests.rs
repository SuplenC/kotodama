use super::*;

fn exact(query: &str, keyword: &str) -> Option<String> {
    named(query, keyword, Typos::Refused, &mut Vec::new()).map(|(rest, naming)| {
        assert_eq!(naming, Naming::Exact);
        rest.to_owned()
    })
}

fn forgiven(query: &str, keyword: &str) -> Option<(String, Naming)> {
    named(query, keyword, Typos::Forgiven, &mut Vec::new()).map(|(rest, naming)| (rest.to_owned(), naming))
}

#[test]
fn a_keyword_is_its_letters_folded_without_separators() {
    assert_eq!(squash("New-Counter"), "newcounter");
    assert_eq!(squash("new_counter"), "newcounter");
    assert_eq!(squash("Żółw zone"), "zolwzone");
    assert!(is_squashed("New-Counter", "newcounter"));
    assert!(!is_squashed("new-counter", "newcount"));
}

#[test]
fn the_keyword_written_any_way_is_named_and_what_follows_is_its_argument() {
    for query in [
        "new-counter diciamo",
        "new counter diciamo",
        "New Counter diciamo",
        "new_counter diciamo",
        "newcounter diciamo",
        "  new   counter   diciamo  ",
    ] {
        assert_eq!(exact(query, "new-counter").as_deref(), Some("diciamo"), "{query}");
    }
}

#[test]
fn the_keyword_alone_has_no_argument() {
    assert_eq!(exact("new counter", "new-counter").as_deref(), Some(""));
    assert_eq!(exact("new counter ", "new-counter").as_deref(), Some(""));
}

#[test]
fn the_argument_keeps_its_own_spacing_and_case() {
    assert_eq!(
        exact("new counter Diciamo  Tutto", "new-counter").as_deref(),
        Some("Diciamo  Tutto")
    );
}

#[test]
fn the_argument_may_repeat_a_word_of_the_keyword() {
    assert_eq!(exact("new counter counter", "new-counter").as_deref(), Some("counter"));
}

#[test]
fn a_keyword_cut_short_is_not_named() {
    assert_eq!(forgiven("new co diciamo", "new-counter"), None);
    assert_eq!(forgiven("new diciamo", "new-counter"), None);
    assert_eq!(forgiven("new", "new-counter"), None);
}

#[test]
fn a_keyword_inside_the_query_is_not_named() {
    assert_eq!(forgiven("make new counter", "new-counter"), None);
}

#[test]
fn a_typo_is_forgiven_only_when_asked() {
    assert_eq!(exact("new-coutner diciamo", "new-counter"), None);
    assert_eq!(
        forgiven("new-coutner diciamo", "new-counter"),
        Some(("diciamo".to_owned(), Naming::Typo))
    );
    assert_eq!(
        forgiven("new coutner diciamo", "new-counter"),
        Some(("diciamo".to_owned(), Naming::Typo))
    );
}

#[test]
fn each_word_allows_a_typo_per_four_letters() {
    // "lck" is three letters, which allow none.
    assert_eq!(forgiven("lck in 10m", "lock-in"), None);
    assert_eq!(
        forgiven("lock on 10m", "lock-in"),
        Some(("10m".to_owned(), Naming::Typo))
    );
}

#[test]
fn the_first_letter_is_never_a_typo() {
    assert_eq!(forgiven("bew counter diciamo", "new-counter"), None);
}

#[test]
fn the_exact_keyword_wins_over_a_shorter_lead_with_a_typo() {
    // "counter" alone is "counters" with a letter missing.
    assert_eq!(
        forgiven("counter s x", "counters"),
        Some(("x".to_owned(), Naming::Exact))
    );
}

#[test]
fn leads_stop_past_the_longest_keyword() {
    let mut seen = Vec::new();
    leads("new counter diciamo", 10, |lead, typed, rest| {
        seen.push((lead.to_owned(), typed.to_owned(), rest.to_owned()));
        false
    });
    assert_eq!(
        seen,
        [
            ("new".to_owned(), "new".to_owned(), "counter diciamo".to_owned()),
            ("newcounter".to_owned(), "new counter".to_owned(), "diciamo".to_owned()),
        ]
    );
}

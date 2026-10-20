use super::*;

/// A Narrowing that remembers `query` matched `matched`.
fn after(query: &str, matched: &[u32]) -> Narrowing<u32> {
    let mut narrowing = Narrowing::default();
    narrowing.found(query, &rank::Query::new(query), matched.iter().copied());
    narrowing
}

fn within<'a>(narrowing: &'a Narrowing<u32>, query: &str) -> Option<&'a [u32]> {
    narrowing.within(query, &rank::Query::new(query))
}

#[test]
fn a_query_that_adds_to_the_last_searches_only_its_matches() {
    let narrowing = after("fire", &[2, 5]);

    assert_eq!(within(&narrowing, "firef"), Some(&[2, 5][..]));
    assert_eq!(within(&narrowing, "fire fox"), Some(&[2, 5][..]));
    assert_eq!(within(&narrowing, "fire"), Some(&[2, 5][..]));
}

#[test]
fn a_query_that_does_not_add_to_the_last_searches_everything() {
    let narrowing = after("fire", &[2, 5]);

    assert_eq!(within(&narrowing, "fir"), None);
    assert_eq!(within(&narrowing, "fore"), None);
}

#[test]
fn nothing_searched_yet_searches_everything() {
    let narrowing = Narrowing::<u32>::default();

    assert_eq!(within(&narrowing, "fire"), None);
}

#[test]
fn a_query_after_one_letter_searches_everything() {
    let narrowing = after("f", &[2]);

    assert_eq!(within(&narrowing, "fi"), None);
}

#[test]
fn a_query_long_enough_for_subtitles_after_one_too_short_searches_everything() {
    let narrowing = after("fi", &[2]);

    assert_eq!(within(&narrowing, "fir"), None);
}

#[test]
fn a_query_with_the_matchers_syntax_searches_everything() {
    let narrowing = after("fire", &[2, 5]);

    for query in ["fire!", "fire$", "fire^", "fire'", "fire\\"] {
        assert_eq!(within(&narrowing, query), None, "{query:?}");
    }
}

#[test]
fn a_forgotten_query_searches_everything() {
    let mut narrowing = after("fire", &[2, 5]);

    narrowing.forget();

    assert_eq!(within(&narrowing, "firef"), None);
    assert!(narrowing.matched().is_empty());
}

#[test]
fn found_replaces_the_last_matches() {
    let mut narrowing = after("fire", &[2, 5]);

    narrowing.found("firef", &rank::Query::new("firef"), [5]);

    assert_eq!(narrowing.matched(), [5]);
}

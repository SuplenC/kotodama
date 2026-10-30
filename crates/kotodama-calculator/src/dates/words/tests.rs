use super::*;

#[test]
fn cleaning_drops_case_apostrophes_and_the_closing_question_mark() {
    assert_eq!(clean("Days 'til New Year’s?"), "days til new years");
    assert_eq!(clean("Christmas!? "), "christmas");
}

#[test]
fn filler_words_are_dropped_and_signs_split_off() {
    assert_eq!(
        words("how many days are there left until the new years"),
        ["how", "many", "days", "until", "new", "years"]
    );
    assert_eq!(words("today+3 weeks"), ["today", "+", "3", "weeks"]);
    assert_eq!(words("2026-12-25 -1 day"), ["2026-12-25", "-", "1", "day"]);
}

#[test]
fn a_long_keyword_is_recognised_through_one_typo() {
    assert!(like("untill", "until"));
    assert!(like("unti", "until"));
    assert!(like("christams", "christmas"));
    assert!(like("chrismas", "christmas"));
    assert!(like("xhristmas", "christmas"));
}

#[test]
fn two_typos_or_a_short_keyword_are_not_recognised() {
    assert!(!like("halowen", "halloween"));
    assert!(!like("unit", "until"));
    assert!(!like("eva", "eve"));
    assert!(!like("unt", "until"));
}

#[test]
fn until_and_since_have_their_synonyms() {
    for word in ["until", "til", "till", "to", "before", "untill"] {
        assert!(until(word), "{word}");
    }
    for word in ["since", "from", "sinse"] {
        assert!(since(word), "{word}");
    }
    assert!(!until("since"));
}

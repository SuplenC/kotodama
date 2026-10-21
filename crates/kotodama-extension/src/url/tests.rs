use super::*;

#[test]
fn spaces_ampersands_and_non_ascii_are_encoded_as_utf_8_bytes() {
    assert_eq!(
        fill("https://example.com/?q={query}", "rock & roll żółw"),
        "https://example.com/?q=rock%20%26%20roll%20%C5%BC%C3%B3%C5%82w"
    );
}

#[test]
fn unreserved_characters_stay_and_the_rest_are_encoded() {
    assert_eq!(percent_encode("a-b.c_d~e"), "a-b.c_d~e");
    assert_eq!(percent_encode("a+b=c?d#e/f%"), "a%2Bb%3Dc%3Fd%23e%2Ff%25");
}

#[test]
fn every_placeholder_is_filled_and_a_template_without_one_is_unchanged() {
    assert_eq!(fill("{query}/{query}", "a b"), "a%20b/a%20b");
    assert_eq!(fill("https://github.com", "x"), "https://github.com");
}

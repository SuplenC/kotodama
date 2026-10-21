use super::*;

#[test]
fn words_of_a_text_that_isnt_ascii_start_where_nucleo_holds_them() {
    // nucleo holds a grapheme as one character: the accent is no
    // character of its own.
    let mut starts = Vec::new();
    starts_and_initials(&fold_text("Cafe\u{301} Menu"), None, &mut starts);
    assert_eq!(starts, [0, 5]);
}

#[test]
fn ascii_text_is_folded_and_split_as_a_character_at_a_time() {
    for c in (0..128u8).map(char::from) {
        assert_eq!(fold_text(&c.to_string()), fold(c).to_string(), "{c:?}");
    }
    let words = [
        "getUserName",
        "VSCode",
        "a",
        "x1",
        "-",
        "Files",
        "web-Storm",
        "HTTPServer",
        "Cafe\u{301}",
        "Żółw",
        "",
        " ",
    ];
    // Texts across a block of bytes, too.
    for n in 0..400usize {
        let text: String = (0..n % 37)
            .map(|k| words[(n * 7 + k * 3) % words.len()])
            .collect::<Vec<_>>()
            .join(["", " ", "_", "\u{1f}"][n % 4]);
        let folded = fold_text(&text);
        assert_eq!(folded, text.chars().map(fold).collect::<String>());
        assert_eq!(Letters::in_str(&folded), Letters::in_text(folded.chars()));
        for original in [Some(text.as_str()), None] {
            let mut starts = Vec::new();
            let initials = starts_and_initials(&folded, original, &mut starts);
            let mut expected = Vec::new();
            word_starts(Utf32String::from(folded.as_str()).slice(..), original, &mut expected);
            assert_eq!(starts, expected, "{text:?}");
            assert_eq!(initials, self::initials(&folded, original), "{text:?}");
        }
    }
}

fn tier(text: &str, query: &str) -> Option<Tier> {
    let chars = folded(text);
    let mut starts = Vec::new();
    word_starts(chars.slice(..), Some(text), &mut starts);
    let text = Text {
        chars: chars.slice(..),
        starts: &starts,
    };
    Tiers::text(text, &Query::new(query))
}

#[test]
fn tiers_from_exact_to_scattered() {
    assert_eq!(tier("Files", "files"), Some(Tier::Exact));
    assert_eq!(tier("Fire Alarm", "fire  alarm"), Some(Tier::Exact));
    assert_eq!(tier("Visual Studio Code", "vsc"), Some(Tier::Initials));
    assert_eq!(tier("Terminal", "term"), Some(Tier::WordStart));
    assert_eq!(tier("WebStorm", "storm"), Some(Tier::WordStart));
    assert_eq!(tier("Determinate", "term"), Some(Tier::Inside));
    assert_eq!(tier("Color Decoder", "code"), Some(Tier::Inside));
    assert_eq!(tier("Color Dev", "code"), Some(Tier::Scattered));
    assert_eq!(tier("Firefox", "alarm"), None);
}

#[test]
fn symbols_before_a_title_do_not_keep_it_from_being_exact() {
    assert_eq!(tier("🔥 Fire", "fire"), Some(Tier::Exact));
    assert_eq!(tier("❤️‍🔥 Heart on fire", "heart on fire"), Some(Tier::Exact));
    assert_eq!(tier("- Fire", "fire"), Some(Tier::Exact));
    assert_eq!(tier("🔥 Fire", "🔥 fire"), Some(Tier::Exact));
    assert_eq!(tier("~/fire", "fire"), Some(Tier::WordStart));
    assert_eq!(tier("Fire 🔥", "fire"), Some(Tier::WordStart));
}

#[test]
fn every_word_must_match_and_the_worst_one_counts() {
    assert_eq!(tier("Fire Alarm", "alarm fire"), Some(Tier::WordStart));
    assert_eq!(tier("Fire Alarm", "fire larm"), Some(Tier::Inside));
    assert_eq!(tier("Firefox", "fire alarm"), None);
}

#[test]
fn one_letter_matches_only_at_a_word_start() {
    assert_eq!(tier("Thunar", "t"), Some(Tier::WordStart));
    assert_eq!(tier("Steam", "t"), None);
}

#[test]
fn a_trailing_space_asks_for_a_whole_word() {
    assert_eq!(tier("Fi Calculator", "fi "), Some(Tier::WordStart));
    assert_eq!(tier("Firefox", "fi "), Some(Tier::Inside));
}

#[test]
fn neither_case_nor_accents_matter() {
    assert_eq!(tier("Firefox", "FI"), Some(Tier::WordStart));
    assert_eq!(tier("Żółw Notes", "zolw"), Some(Tier::WordStart));
    assert_eq!(tier("Zolw Notes", "żółw"), Some(Tier::WordStart));
    assert_eq!(fold_text("ŻÓŁW"), "zolw");
}

use kotodama_protocol::ItemKind;

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

/// How well `query` matches an Item titled `Untitled` with `keyword` and
/// `keywords`.
fn keyword_tier(keyword: Option<&str>, keywords: &[&str], query: &str) -> Option<Tier> {
    weak_tier(keyword, keywords, &[], query)
}

/// How well `query` matches an Item titled `Untitled` with `keyword`,
/// `keywords` and `weak_keywords`.
fn weak_tier(keyword: Option<&str>, keywords: &[&str], weak_keywords: &[&str], query: &str) -> Option<Tier> {
    let item = Item {
        keyword: keyword.map(str::to_owned),
        keywords: keywords.iter().map(|&word| word.to_owned()).collect(),
        weak_keywords: weak_keywords.iter().map(|&word| word.to_owned()).collect(),
        ..untitled()
    };
    let mut tiers = Tiers::default();
    tiers.texts(Haystack::new(&item).texts(), &Query::new(query), Matched::ALL)
}

/// An Item titled `Untitled` with nothing else to find it by.
fn untitled() -> Item {
    Item {
        id: "untitled".into(),
        kind: ItemKind::Url { url: "x".into() },
        title: "Untitled".into(),
        subtitle: None,
        icon: None,
        keyword: None,
        identity: None,
        keywords: vec![],
        hidden: false,
        app_only: false,
        actions: vec![],
        weak_keywords: vec![],
        yields: false,
    }
}

#[test]
fn only_the_items_own_keyword_matches_as_exactly_as_a_title() {
    assert_eq!(keyword_tier(Some("go"), &["golang"], "go"), Some(Tier::Keyword));
    assert_eq!(keyword_tier(Some("g"), &[], "g"), Some(Tier::Keyword));
    assert_eq!(keyword_tier(Some("go"), &["golang"], "golang"), Some(Tier::WordStart));
    assert_eq!(keyword_tier(None, &["chrome"], "chrome"), Some(Tier::WordStart));
    assert_eq!(keyword_tier(None, &["c"], "c"), Some(Tier::WordStart));
    assert_eq!(
        keyword_tier(None, &["google", "chrome"], "chrome"),
        Some(Tier::WordStart)
    );
    assert_eq!(keyword_tier(Some(""), &["chrome"], "chrome"), Some(Tier::WordStart));
}

#[test]
fn a_weak_keyword_matches_as_the_query_inside_a_word_at_best() {
    let id = ["chrome-abcdef-Default"];
    assert_eq!(
        weak_tier(None, &[], &id, "chrome-abcdef-default"),
        Some(Tier::WeakKeyword)
    );
    assert_eq!(weak_tier(None, &[], &id, "chrome"), Some(Tier::WeakKeyword));
    assert_eq!(weak_tier(None, &[], &id, "abcdef"), Some(Tier::WeakKeyword));
    assert_eq!(
        weak_tier(None, &[], &id, "crmdflt"),
        None,
        "letters scattered across an id find nothing"
    );
    assert_eq!(
        weak_tier(None, &[], &id, "c"),
        None,
        "one letter matches only at a word start"
    );
    assert_eq!(weak_tier(None, &[], &["x"], "x"), None);
    assert_eq!(weak_tier(None, &[], &["one", "two"], "two"), Some(Tier::WeakKeyword));
    assert_eq!(weak_tier(None, &[], &[""], "untitled"), Some(Tier::Exact));
}

#[test]
fn a_query_may_match_a_keyword_with_one_word_and_a_weak_keyword_with_another() {
    let nautilus = ["org.gnome.Nautilus"];
    assert_eq!(
        weak_tier(None, &["File Manager"], &nautilus, "file nautilus"),
        Some(Tier::WeakKeyword)
    );
    assert_eq!(
        weak_tier(None, &["File Manager"], &nautilus, "file nautilus gedit"),
        None
    );
}

#[test]
fn keywords_keep_their_word_starts_where_nucleo_holds_them_beside_weak_keywords_that_arent_ascii() {
    let item = Item {
        keywords: vec!["one\r\ntwo".into()],
        weak_keywords: vec!["żółw".into()],
        ..untitled()
    };
    let folded = Folded::new(&item);
    let chars: Vec<char> = folded.words.slice(..).chars().collect();

    assert_eq!(chars[folded.weak_at.unwrap() as usize], WEAK);
    assert_eq!(
        folded
            .words_starts
            .iter()
            .map(|&at| chars[at as usize])
            .collect::<String>(),
        "ot"
    );
}

#[test]
fn a_weak_keyword_leaves_the_other_keywords_and_the_title_as_they_match() {
    let id = ["chrome-abcdef-Default"];
    assert_eq!(weak_tier(Some("go"), &[], &id, "go"), Some(Tier::Keyword));
    assert_eq!(weak_tier(None, &["chrome"], &id, "chrome"), Some(Tier::WordStart));
    assert_eq!(
        weak_tier(Some("go"), &["browser"], &id, "browser"),
        Some(Tier::WordStart)
    );
    assert_eq!(weak_tier(None, &["web"], &id, "eb"), Some(Tier::Inside));
    assert_eq!(weak_tier(None, &[], &id, "untitled"), Some(Tier::Exact));
}

use super::*;

#[test]
fn the_comma_languages_are_sorted_for_the_binary_search() {
    assert!(COMMA_LANGUAGES.is_sorted());
}

#[test]
fn a_locale_writes_decimals_as_its_language_does() {
    for locale in [
        "de_DE.UTF-8",
        "pl_PL.UTF-8",
        "fr_FR",
        "pt_BR.UTF-8",
        "ru_RU.KOI8-R",
        "nl_NL@euro",
    ] {
        assert_eq!(DecimalSeparator::of_locale(locale), DecimalSeparator::Comma, "{locale}");
    }
    for locale in [
        "en_US.UTF-8",
        "en_GB.UTF-8",
        "ja_JP.UTF-8",
        "zh_CN.UTF-8",
        "C",
        "POSIX",
        "C.UTF-8",
        "",
    ] {
        assert_eq!(DecimalSeparator::of_locale(locale), DecimalSeparator::Dot, "{locale}");
    }
}

#[test]
fn a_territory_may_write_decimals_unlike_its_language() {
    assert_eq!(DecimalSeparator::of_locale("de_CH.UTF-8"), DecimalSeparator::Dot);
    assert_eq!(DecimalSeparator::of_locale("es_MX.UTF-8"), DecimalSeparator::Dot);
    assert_eq!(DecimalSeparator::of_locale("es_ES.UTF-8"), DecimalSeparator::Comma);
    assert_eq!(DecimalSeparator::of_locale("en_DK.UTF-8"), DecimalSeparator::Comma);
}

#[test]
fn the_setting_names_either_separator() {
    assert_eq!(DecimalSeparator::named("dot"), Some(DecimalSeparator::Dot));
    assert_eq!(DecimalSeparator::named("comma"), Some(DecimalSeparator::Comma));
    assert_eq!(DecimalSeparator::named(","), None);
}

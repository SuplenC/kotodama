use super::*;

#[test]
fn rounding_drops_trailing_zeros() {
    assert_eq!(round("26.0975900740 mi", b'.'), "26.097590074 mi");
    assert_eq!(round("2.5000000000", b'.'), "2.5");
    assert_eq!(round("3.0000000000 kg", b'.'), "3 kg");
    assert_eq!(round("0.0000000000", b'.'), "0");
}

#[test]
fn rounding_keeps_twelve_significant_digits() {
    assert_eq!(round("333333.3333333333", b'.'), "333333.333333");
    assert_eq!(round("666666.6666666667", b'.'), "666666.666667");
    assert_eq!(round("3.1415926536", b'.'), "3.1415926536");
    assert_eq!(round("0.0000003333", b'.'), "0.0000003333");
    assert_eq!(round("1234567890123.75", b'.'), "1234567890124");
}

#[test]
fn rounding_carries_into_the_whole_part() {
    assert_eq!(round("99999.99999999999", b'.'), "100000");
    assert_eq!(round("0.99999999999999", b'.'), "1");
    assert_eq!(round("-19.999999999999 °C", b'.'), "-20 °C");
}

#[test]
fn rounding_reads_every_number_with_the_decimal_separator_given() {
    assert_eq!(
        round("0.5000000000 + 0.8660254038i", b'.'),
        "0.5 + 0.8660254038i"
    );
    assert_eq!(round("26,0975900740 mi", b','), "26,097590074 mi");
    assert_eq!(round("26.0975900740 mi", b','), "26.0975900740 mi");
}

#[test]
fn thousands_separators_stand_between_groups_of_a_thousand() {
    let dot = DecimalSeparator::Dot;
    for query in [
        "1,000",
        "1,000 + 1",
        "12,345.5 * 2",
        "1,000,000 / 3",
        "2+2",
        "(1, 2)",
    ] {
        assert!(grouped_in_thousands(query, dot), "{query}");
    }
    for query in [
        "3,5 * 2",
        "1,00 + 1",
        "1000,000",
        "1,0000",
        "1.000,5 + 1",
        "1,2,3",
    ] {
        assert!(!grouped_in_thousands(query, dot), "{query}");
    }
}

#[test]
fn with_a_decimal_comma_dots_group_the_thousands() {
    let comma = DecimalSeparator::Comma;
    for query in ["3,5 * 2", "1.000 + 1", "1.000.000,5 / 2"] {
        assert!(grouped_in_thousands(query, comma), "{query}");
    }
    for query in ["3.5 * 2", "1,000.5 + 1", "0.1 + 0.2"] {
        assert!(!grouped_in_thousands(query, comma), "{query}");
    }
}

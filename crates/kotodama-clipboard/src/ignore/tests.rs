use super::*;

#[test]
fn by_default_one_time_codes_and_secret_tokens_are_left_out() {
    let ignore = Ignore::default();
    // Each token is made-up and split in two, so secret scanners, which match
    // the same shapes, don't take the source for a leak.
    let secrets = [
        "482913",
        "482 913",
        "482-913\n",
        "  048291  ",
        "-----BEGIN OPENSSH PRIVATE KEY-----\nb3BlbnNzaC1rZXktdjEAAAAA\n-----END OPENSSH PRIVATE KEY-----",
        "-----BEGIN RSA PRIVATE KEY-----",
        concat!("ghp_", "aBcDeFgHiJkLmNoPqRsTuVwXyZ0123456789"),
        concat!("GITHUB_TOKEN=gho_", "aBcDeFgHiJkLmNoPqRsTuVwXyZ0123456789"),
        concat!(
            "github_pat_",
            "11ABCDEFG0123456789_abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXY"
        ),
        concat!("glpat-", "aBcDeFgHiJkLmNoPqRsT"),
        concat!("xoxb-", "1234567890-abcdefghijkl"),
        concat!("AKIA", "IOSFODNN7EXAMPLE"),
        concat!("sk_live_", "aBcDeFgHiJkLmNoP0123"),
        concat!("sk-proj-", "aBcDeFgHiJkLmNoPqRsTuVwXyZ012345"),
        concat!("sk-ant-api03-", "aBcDeFgHiJkLmNoPqRsTuVwXyZ0123456789"),
        concat!(
            "Bearer eyJhbGciOiJIUzI1NiJ9.",
            "eyJzdWIiOiIxMjM0NTY3ODkwIn0.dozjgNryP4J3jVmNHl0w5N_XgL0n3I9PlFUP0THsR8U"
        ),
    ];

    let kept: Vec<_> = secrets.into_iter().filter(|text| !ignore.ignores(text)).collect();

    assert_eq!(kept, [] as [&str; 0]);
}

#[test]
fn by_default_ordinary_text_is_kept() {
    let ignore = Ignore::default();
    let ordinary = [
        "hello",
        "42",
        "2026",
        "12345",
        "1234567",
        "20261021",
        "123456 apples",
        "Order #123456 has shipped",
        "+48 600 700 800",
        "#123456",
        "3.14159",
        "https://example.com/?page=123456",
        "4b825dc642cb6eb9a060e54bf8d69288fbee4904",
        "ghp_short",
        "sk-learn",
        "risk_live_well",
        "AKIA",
        "-----BEGIN CERTIFICATE-----",
        "-----BEGIN PUBLIC KEY-----",
        "eyJhbGciOiJIUzI1NiJ9",
        "Meeting at 10:30, room 4.12",
    ];

    let left_out: Vec<_> = ordinary.into_iter().filter(|text| ignore.ignores(text)).collect();

    assert_eq!(left_out, [] as [&str; 0]);
}

#[test]
fn no_patterns_keep_everything() {
    let (ignore, errors) = Ignore::new([]);

    assert!(errors.is_empty());
    assert!(!ignore.ignores("482913"));
}

#[test]
fn the_configured_patterns_replace_the_defaults() {
    let (ignore, errors) = Ignore::new(["^secret:"]);

    assert!(errors.is_empty());
    assert!(ignore.ignores("secret: hunter2"));
    assert!(!ignore.ignores("482913"));
}

#[test]
fn a_pattern_that_isnt_a_regular_expression_is_left_out_and_the_rest_apply() {
    let (ignore, errors) = Ignore::new(["(unclosed", "^pin [0-9]{4}$"]);

    assert!(matches!(&errors[..], [PatternError::Invalid { pattern, .. }] if pattern == "(unclosed"));
    assert!(ignore.ignores("pin 1234"));
}

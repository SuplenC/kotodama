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

    assert!(matches!(&errors[..], [error] if error.pattern() == "(unclosed"));
    assert!(ignore.ignores("pin 1234"));
}

/// `DEFAULT_IGNORE` compiled, one by one.
fn expressions() -> Vec<Regex> {
    DEFAULT_IGNORE
        .iter()
        .map(|expression| Regex::new(expression).unwrap())
        .collect()
}

/// `ignore` set to `DEFAULT_IGNORE`.
fn configured_defaults() -> Ignore {
    let (ignore, errors) = Ignore::new(DEFAULT_IGNORE);
    assert!(errors.is_empty(), "{errors:?}");
    ignore
}

#[test]
fn the_defaults_match_what_their_expressions_match() {
    let expressions = expressions();
    // Pieces of the defaults' shapes, cut short or run long, around the
    // bytes that end a word, a part or a line.
    let pieces = [
        "",
        " ",
        "  ",
        "\t",
        "\n",
        "\x0b",
        "\x0c",
        "\r",
        "-",
        "_",
        ".",
        "..",
        "=",
        "é",
        "x",
        "Z",
        "7",
        "0",
        "123",
        "456",
        "1234",
        "123 ",
        "123-",
        "-----BEGIN ",
        "RSA ",
        "OPENSSH ",
        "ENCRYPTED  ",
        "rsa ",
        "PRIVATE KEY-----",
        "PRIVATE KEY",
        "PUBLIC KEY-----",
        "ghp_",
        "gho_",
        "ghx_",
        "github_pat_",
        "glpat-",
        "xoxb-",
        "xoxz-",
        "AKIA",
        "ASIA",
        "sk_live_",
        "rk_test_",
        "sk_prod_",
        "sk-",
        "sk-ant-",
        "eyJ",
        "eyJ-",
        ".eyJ",
        "eyJaBcDeFgH",
        "eyJaBcDeFg",
        ".eyJaBcDeFgH.",
        ".eyJaBcDeFg.",
        "aBcDeFg",
        "aBcDeFgH",
        "ABCDEFGH",
        "01234567",
        "abc-_def",
        "aBcDeFgHiJkLmNoPqRsTuVwXyZ0123456789",
        "aBcDeFgHiJkLmNoPqRsTuVwXyZ_0123456789-aBcDeFgHiJkLmNoPqRsTuVwX",
        "IOSFODNN7EXAMPLE",
        "IOSFODNN7EXAMPL",
        "IOSFODNN7EXAMPLEX",
    ];
    let mut seed: u32 = 0x2545_f491;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed as usize
    };

    let mut matched = [false; DEFAULT_IGNORE.len()];

    for _ in 0..200_000 {
        let text: String = (0..=next() % 6).map(|_| pieces[next() % pieces.len()]).collect();

        let mut any = false;
        for (matched, expression) in matched.iter_mut().zip(&expressions) {
            let hit = expression.is_match(&text);
            *matched |= hit;
            any |= hit;
        }
        assert_eq!(Ignore::default().ignores(&text), any, "{text:?}");
    }
    assert_eq!(
        matched,
        [true; DEFAULT_IGNORE.len()],
        "each expression matches some text"
    );
}

#[test]
fn the_defaults_match_a_long_run_of_token_starts_as_their_expressions_do() {
    let configured = configured_defaults();
    let texts = [
        "sk-".repeat(100_000),
        "eyJ-".repeat(100_000),
        format!("{}.eyJ{}.", "eyJ-".repeat(50_000), "a".repeat(8)),
        "ghp_".repeat(100_000),
        "-----BEGIN ".repeat(100_000),
        "-----BEGIN A ".repeat(100_000),
        format!("{}{}", "eyJ.".repeat(100_000), "x".repeat(8)),
    ];

    for text in texts {
        assert_eq!(
            Ignore::default().ignores(&text),
            configured.ignores(&text),
            "{}",
            &text[..40]
        );
    }
}

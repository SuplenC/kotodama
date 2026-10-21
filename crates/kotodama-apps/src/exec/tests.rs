use super::*;

const FIELDS: Fields = Fields {
    name: "Firefox",
    icon: Some("firefox"),
    file: "/usr/share/applications/firefox.desktop",
};

fn run(exec: &str) -> Vec<String> {
    command(exec, &FIELDS).unwrap()
}

#[test]
fn file_and_url_codes_are_dropped() {
    assert_eq!(run("firefox %u"), ["firefox"]);
    assert_eq!(run("gimp-2.10 %U --new"), ["gimp-2.10", "--new"]);
}

#[test]
fn quoted_arguments_keep_their_spaces_and_escapes() {
    assert_eq!(
        run(r#"sh -c "echo \"hi\" \$HOME \\ \`x\`" end"#),
        ["sh", "-c", r#"echo "hi" $HOME \ `x`"#, "end"]
    );
    assert_eq!(run(r#""/opt/My App/app"  --flag"#), ["/opt/My App/app", "--flag"]);
}

#[test]
fn icon_name_and_file_codes_expand() {
    assert_eq!(
        run("app %i --title=%c --from %k 100%%"),
        [
            "app",
            "--icon",
            "firefox",
            "--title=Firefox",
            "--from",
            "/usr/share/applications/firefox.desktop",
            "100%"
        ]
    );
    let fields = Fields { icon: None, ..FIELDS };
    assert_eq!(command("app %i", &fields).unwrap(), ["app"]);
}

#[test]
fn an_unclosed_quote_or_nothing_to_run_is_an_error() {
    assert!(command(r#"app "open"#, &FIELDS).is_err());
    assert!(command("  %U ", &FIELDS).is_err());
}

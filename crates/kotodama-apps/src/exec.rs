//! Turning an `Exec` value into the program and arguments to run, as the
//! Desktop Entry Specification describes: quoting, then field codes.

/// What the field codes expand to. Kotodama starts apps with no files or URLs,
/// so `%f`, `%F`, `%u` and `%U` expand to nothing.
pub struct Fields<'a> {
    /// The translated name, for `%c`.
    pub name: &'a str,
    /// For `%i`, which becomes `--icon <icon>`, or nothing without one.
    pub icon: Option<&'a str>,
    /// The `.desktop` file's path, for `%k`.
    pub file: &'a str,
}

/// Splits `exec`, with the string escapes already undone, into arguments and
/// expands the field codes. Fails on an unclosed quote or an empty command.
pub fn command(exec: &str, fields: &Fields) -> Result<Vec<String>, String> {
    let mut argv = Vec::new();
    for arg in split(exec)? {
        match arg.as_str() {
            "%f" | "%F" | "%u" | "%U" | "%d" | "%D" | "%n" | "%N" | "%v" | "%m" => {}
            "%i" => {
                if let Some(icon) = fields.icon {
                    argv.push("--icon".to_owned());
                    argv.push(icon.to_owned());
                }
            }
            _ => argv.push(expand(&arg, fields)),
        }
    }
    if argv.is_empty() {
        return Err("`Exec` is empty".into());
    }
    Ok(argv)
}

/// Splits on unquoted whitespace. Inside double quotes, a backslash escapes
/// `"`, `` ` ``, `$` and `\`.
fn split(exec: &str) -> Result<Vec<String>, String> {
    let mut args = Vec::new();
    let mut arg: Option<String> = None;
    let mut chars = exec.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                let arg = arg.get_or_insert_with(String::new);
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') => match chars.next() {
                            Some(c @ ('"' | '`' | '$' | '\\')) => arg.push(c),
                            Some(c) => {
                                arg.push('\\');
                                arg.push(c);
                            }
                            None => return Err("`Exec` ends inside a quote".into()),
                        },
                        Some(c) => arg.push(c),
                        None => return Err("`Exec` ends inside a quote".into()),
                    }
                }
            }
            c if c.is_whitespace() => {
                if let Some(arg) = arg.take() {
                    args.push(arg);
                }
            }
            c => arg.get_or_insert_with(String::new).push(c),
        }
    }
    args.extend(arg);
    Ok(args)
}

/// Expands the field codes inside one argument: `%%` is `%`, `%c` and `%k` as
/// [`Fields`] says, and any other code is dropped.
fn expand(arg: &str, fields: &Fields) -> String {
    if !arg.contains('%') {
        return arg.to_owned();
    }
    let mut out = String::with_capacity(arg.len());
    let mut chars = arg.chars();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('%') => out.push('%'),
            Some('c') => out.push_str(fields.name),
            Some('k') => out.push_str(fields.file),
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
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
}

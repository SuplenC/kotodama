//! Turning an `Exec` value into the program and arguments to run, as the
//! Desktop Entry Specification describes: quoting, then field codes.
//!
//! # Owns
//!
//! - Splitting `Exec` into arguments and expanding its field codes.
//!
//! # Not here
//!
//! - The string escapes, which reading the file undoes first: `entry.rs`.
//! - Starting the command: `kotodama_extension::launch`, from `main.rs`.

/// What the field codes expand to. Kotodama starts apps with no files or URLs,
/// so `%f`, `%F`, `%u` and `%U` expand to nothing.
#[derive(Debug)]
pub struct Fields<'a> {
    /// The translated name, for `%c`.
    pub name: &'a str,
    /// For `%i`, which becomes `--icon <icon>`, or nothing without one.
    pub icon: Option<&'a str>,
    /// The `.desktop` file's path, for `%k`.
    pub file: &'a str,
}

/// Why an `Exec` value is no command to run.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("`Exec` ends inside a quote")]
    UnclosedQuote,
    #[error("`Exec` is empty")]
    Empty,
}

/// Splits `exec`, with the string escapes already undone, into arguments and
/// expands the field codes.
///
/// # Errors
///
/// [`Error::UnclosedQuote`] on a quote left open, [`Error::Empty`] when no
/// argument is left once the field codes expand.
pub fn command(exec: &str, fields: &Fields) -> Result<Vec<String>, Error> {
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
        return Err(Error::Empty);
    }
    Ok(argv)
}

/// Splits on unquoted whitespace. Inside double quotes, a backslash escapes
/// `"`, `` ` ``, `$` and `\`.
fn split(exec: &str) -> Result<Vec<String>, Error> {
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
                            None => return Err(Error::UnclosedQuote),
                        },
                        Some(c) => arg.push(c),
                        None => return Err(Error::UnclosedQuote),
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
mod tests;

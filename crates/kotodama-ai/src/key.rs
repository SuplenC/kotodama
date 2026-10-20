//! The API key.
//!
//! # Owns
//!
//! - Running `api_key_command` and reading the key from its output.
//! - Keeping the key out of every message: [`Secret`] never prints it.
//!
//! # Not here
//!
//! - Handing the key to `curl`: `request.rs`, on its stdin.

use std::fmt;
use std::io;
use std::process::Command;
use std::process::ExitStatus;
use std::process::Stdio;

/// An API key, kept in memory only. Its `Debug` hides it.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    #[must_use]
    pub fn new(key: impl Into<String>) -> Self {
        Secret(key.into())
    }

    pub(crate) fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(<redacted>)")
    }
}

/// Why `api_key_command` gave no key. No variant holds what it printed.
#[derive(Debug, thiserror::Error)]
pub enum KeyError {
    #[error("`api_key_command` is empty")]
    Empty,
    #[error("can't run `api_key_command` ({program})")]
    Start {
        program: String,
        #[source]
        source: io::Error,
    },
    #[error("`api_key_command` failed: {0}")]
    Failed(ExitStatus),
    #[error("`api_key_command` printed no key")]
    NoKey,
}

/// Runs `api_key_command` and reads the key from its first line. Its stderr
/// is dropped, as a password manager may print there.
///
/// # Errors
///
/// When the command is empty, can't start, fails, or prints no key.
pub fn read_key(command: &[String]) -> Result<Secret, KeyError> {
    let Some((program, args)) = command.split_first() else {
        return Err(KeyError::Empty);
    };
    let output = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(|source| KeyError::Start {
            program: program.clone(),
            source,
        })?;
    if !output.status.success() {
        return Err(KeyError::Failed(output.status));
    }
    let Ok(key) = String::from_utf8(output.stdout) else {
        return Err(KeyError::NoKey);
    };
    match key.lines().next().map(str::trim) {
        Some(key) if !key.is_empty() => Ok(Secret::new(key)),
        _ => Err(KeyError::NoKey),
    }
}

#[cfg(test)]
mod tests;

//! `kotodama check`: prints what the Daemon found wrong that the user can fix, one problem a line,
//! and exits with 1 when there is any, so a script or a login hook can tell, or with 2 when it
//! can't ask the Daemon.
//!
//! # Owns
//!
//! - Reading the problems from the Daemon's first `ToFrontEnd::Status`, and printing them.
//!
//! # Not here
//!
//! - Finding them: the Daemon, `kotodamad/src/problem.rs`.
//! - Connecting to the Daemon: `main.rs`, `connect`.

use std::io;
use std::io::Read;
use std::io::Write;
use std::process::ExitCode;

use kotodama_protocol::Channel;
use kotodama_protocol::FromFrontEnd;
use kotodama_protocol::Problem;
use kotodama_protocol::ToFrontEnd;

use crate::SendError;

/// Runs `kotodama check`.
pub fn main() -> ExitCode {
    let problems = crate::connect().and_then(|mut channel| problems(&mut channel));
    match problems {
        Ok(problems) => {
            let mut out = io::stdout().lock();
            // A closed stdout, as in `kotodama check | head -1`, still leaves the exit code.
            let _ = print(&problems, &mut out);
            if problems.is_empty() {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Err(err) => {
            eprintln!("kotodama: {err}");
            ExitCode::from(2)
        }
    }
}

/// The problems the Daemon lists as the command connects. A request to close is answered at once,
/// as `answer` does, so typing into the window underneath never waits on the command.
pub fn problems<R: Read, W: Write>(channel: &mut Channel<R, W>) -> Result<Vec<Problem>, SendError> {
    loop {
        match channel.recv().map_err(SendError::answer)? {
            Some(ToFrontEnd::Status { problems, .. }) => return Ok(problems),
            Some(ToFrontEnd::Close) => channel.send(&FromFrontEnd::Closed).map_err(SendError::send)?,
            Some(_) => {}
            None => return Err(SendError::HungUp),
        }
    }
}

/// Writes each problem on a line of its own, by what it is about, or that there are none.
pub fn print(problems: &[Problem], out: &mut impl Write) -> io::Result<()> {
    if problems.is_empty() {
        return writeln!(out, "No problems.");
    }
    for problem in problems {
        writeln!(out, "{}: {}", problem.about, problem.message)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::os::unix::net::UnixStream;

    use kotodama_protocol::Hello;
    use kotodama_protocol::WireFormat;

    use super::*;

    fn problem(about: &str, message: &str) -> Problem {
        Problem {
            about: about.into(),
            message: message.into(),
        }
    }

    fn printed(problems: &[Problem]) -> String {
        let mut out = Vec::new();
        print(problems, &mut out).unwrap();
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn each_problem_is_a_line_by_what_it_is_about() {
        let problems = [
            problem("history.half_life_days", "should be a number of days above 0"),
            problem("extension calculator", "stopped (exit status: 1); restarting"),
        ];

        assert_eq!(
            printed(&problems),
            "history.half_life_days: should be a number of days above 0\n\
             extension calculator: stopped (exit status: 1); restarting\n"
        );
    }

    #[test]
    fn no_problems_says_so() {
        assert_eq!(printed(&[]), "No problems.\n");
    }

    #[test]
    fn the_problems_are_those_of_the_first_status() {
        let (ours, theirs) = UnixStream::pair().unwrap();
        let daemon = std::thread::spawn(move || {
            let reader = theirs.try_clone().unwrap();
            let mut channel = Channel::handshake(reader, theirs, &Hello::new(&WireFormat::ALL)).unwrap();
            channel.send(&ToFrontEnd::Close).unwrap();
            assert_eq!(channel.recv::<FromFrontEnd>().unwrap(), Some(FromFrontEnd::Closed));
            let problems = vec![problem("keyword `gh`", "two things ask for it")];
            channel
                .send(&ToFrontEnd::Status {
                    down: Vec::new(),
                    problems,
                })
                .unwrap();
        });
        let reader = ours.try_clone().unwrap();
        let mut channel = Channel::handshake(reader, ours, &Hello::new(&WireFormat::ALL)).unwrap();

        let problems = problems(&mut channel).unwrap();
        daemon.join().unwrap();

        assert_eq!(problems, [problem("keyword `gh`", "two things ask for it")]);
    }
}

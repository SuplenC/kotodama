//! The emoji Bundled extension: every emoji and common symbol as an Indexed
//! item, sent once at start, so the Daemon finds them as it finds apps, with
//! Frecency. The primary Action asks the Daemon to type the character; the
//! second copies it; an emoji with skin tones has an Action to type each.
//!
//! ```toml
//! [extensions.emoji]
//! # The skin tone typing and copying use: light, medium-light, medium,
//! # medium-dark or dark. Unset, emoji are their default yellow.
//! skin_tone = "medium"
//! ```

use std::process::ExitCode;

use kotodama_emoji::Tone;
use kotodama_extension::{Connection, Event, config};
use kotodama_protocol::{FromExtension, Table, ToExtension};

fn main() -> ExitCode {
    kotodama_extension::main("kotodama-emoji", run)
}

fn run() -> Result<(), kotodama_protocol::Error> {
    let Connection {
        mut writer,
        received,
        ..
    } = kotodama_extension::connect::<()>()?;
    // The characters never change, so they are sent once, whatever the config.
    writer.send(&FromExtension::AddItems {
        items: kotodama_emoji::items(),
    })?;
    let mut tone = None;
    for event in received {
        match event {
            Event::Message(ToExtension::Config(table)) => tone = skin_tone(&table),
            Event::Message(ToExtension::RunAction {
                invocation,
                action,
                item,
                ..
            }) => writer.send(&FromExtension::ActionDone {
                invocation,
                outcome: kotodama_emoji::outcome(&item.id, &action, tone),
            })?,
            Event::Message(_) | Event::Own(()) => {}
            Event::Hangup => break,
        }
    }
    Ok(())
}

/// The `skin_tone` setting; one that names no tone is logged and left unset.
fn skin_tone(table: &Table) -> Option<Tone> {
    let name = config::string(table, "skin_tone")?;
    let tone = Tone::from_name(name);
    if tone.is_none() {
        config::warn(format!(
            "skin_tone: `{name}` isn't light, medium-light, medium, medium-dark or dark"
        ));
    }
    tone
}

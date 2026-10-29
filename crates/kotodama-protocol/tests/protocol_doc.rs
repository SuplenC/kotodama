//! Writes `docs/protocol.md`, the protocol message by message, from the JSON
//! samples pinned in `layouts/<version>.txt` for the current version, and
//! fails when the file in the repository isn't what it writes. Rerun with
//! `KOTODAMA_BLESS_LAYOUT=1` to rewrite it, as for the layout and
//! `kotodama.proto`.
//!
//! Each sample has a sentence or two here saying what the message is for. A
//! sample without one fails the test, so a new message is documented in the
//! change that adds it.

use std::fmt::Write as _;
use std::path::Path;

use kotodama_protocol::PROTOCOL_VERSION;

const BLESS: &str = "KOTODAMA_BLESS_LAYOUT";

/// The groups of samples in the order the document lists them, with a
/// heading and an introduction each.
const SECTIONS: [(&str, &str, &str); 11] = [
    (
        "ToExtension",
        "From the Daemon to an Extension",
        "What an Extension reads. Ignore a message you don't handle: a newer Daemon may send more, though \
         never a message or a field newer than the version your hello names.",
    ),
    (
        "FromExtension",
        "From an Extension to the Daemon",
        "What an Extension writes. An optional field can be left out; [extensions.md](extensions.md) says which \
         fields each message needs.",
    ),
    (
        "Outcome",
        "Outcomes",
        "How an Action finished: the `outcome` of `action_done`.",
    ),
    (
        "Clipboard",
        "Clipboard contents",
        "What `paste` and `copy` put on the clipboard.",
    ),
    (
        "Item",
        "Items, by kind",
        "An Item is flat: its kind's one field sits beside `kind`. `id`, `kind`, `title` and at least one Action \
         are required; the shared kinds also need their field. Leave out the fields of other kinds, or send them \
         as `null`.",
    ),
    (
        "ViewContent",
        "View contents",
        "The `content` of a View an Extension pushes, updates or replaces.",
    ),
    (
        "FormInput",
        "Form inputs",
        "The `input` of a form field, which also decides the type of its submitted value.",
    ),
    (
        "JobState",
        "Job states",
        "The `state` of a job an Extension reports with `job_progress`.",
    ),
    (
        "Value",
        "Config values",
        "What a config table and a submitted form hold: anything TOML can hold, untagged in JSON. Dates arrive \
         as strings.",
    ),
    (
        "FromFrontEnd",
        "From a Front end to the Daemon",
        "Extensions never see these; they are here for whoever writes a Front end.",
    ),
    (
        "ToFrontEnd",
        "From the Daemon to a Front end",
        "Extensions never see these either.",
    ),
];

/// What each sample is for, by its label in the layout.
fn describe(label: &str) -> Option<&'static str> {
    if label.starts_with("ToExtension::") || label.starts_with("FromExtension::") {
        return describe_extension_message(label);
    }
    Some(match label {
        "Outcome::close" => "Done; the menu closes.",
        "Outcome::keep_open" => "Done; the menu stays open, as after pushing a View.",
        "Outcome::type" => "The menu closes and the Daemon types `text` into the window that had focus.",
        "Outcome::paste" => "The menu closes and the Daemon pastes into the window that had focus.",
        "Outcome::copy" => "The menu closes and the Daemon puts this on the clipboard, without pasting.",
        "Outcome::failed" => "The Action failed; the menu stays open and shows `message`.",
        "Outcome::run_catalog_action" => {
            "Runs an Action from the Action catalog, as if the user had picked it; only for an Extension whose \
             Manifest reads the catalog, as the outcome of an Action the user ran."
        }
        "Clipboard::text" => "Text.",
        "Clipboard::image" => "An image: its MIME type, and its bytes, base64 in JSON and MessagePack.",
        "Item::App" => "An application, by its desktop file id.",
        "Item::File" => "A file, by its absolute path.",
        "Item::Folder" => "A folder, by its absolute path.",
        "Item::Project" => "A project's folder, by its absolute path.",
        "Item::Window" => "An open window, by the compositor's address for it.",
        "Item::Url" => "A web address.",
        "Item::Namespaced" => {
            "Any other kind belongs to one Extension and is named `<extension>.<kind>`, without a field of its \
             own."
        }
        "ViewContent::list" => {
            "Items to pick from, whose Actions run like a Result's. Typing filters them in the Daemon, unless \
             `filters_itself` is set: then the Extension gets `filter_view`."
        }
        "ViewContent::form" => "Fields to fill in, and the submit button's label; submitting sends `submit_form`.",
        "ViewContent::detail" => "Markdown, with label and value pairs beside it.",
        "ViewContent::progress" => "Follows the job with this id as the Extension reports it.",
        "FormInput::text" => "A line of text; `secret` hides what is typed. Submitted as a string.",
        "FormInput::checkbox" => "A checkbox. Submitted as a boolean.",
        "FormInput::select" => "One of a list of options. Submitted as the option's `value`.",
        "FormInput::file" => {
            "A path to a file, or with `folder` to a folder; `accept` lists the extensions or MIME types a file may \
             have. Submitted as a string, the path made absolute by the Daemon."
        }
        "JobState::running" => "Still going.",
        "JobState::succeeded" => "Done.",
        "JobState::failed" => "Gave up; the `message` says why.",
        "Value::Bool" => "A boolean.",
        "Value::Integer" => "A whole number.",
        "Value::Float" => "A number with a fraction.",
        "Value::String" => "A string.",
        "Value::Array" => "A list of values.",
        "Value::Table" => "A table of values by key.",
        "FromFrontEnd::query" => "The search text changed.",
        "FromFrontEnd::viewport" => "Which Results are on screen: `rows` of them from `top`.",
        "FromFrontEnd::selection_moved" => "The user moved the selection off the top row.",
        "FromFrontEnd::run_action" => {
            "Runs an Action of a Result, naming the `Results` version on screen when the user chose it."
        }
        "FromFrontEnd::submit_form" => "The user submitted a form View.",
        "FromFrontEnd::view_popped" => "The user left a View, as with Esc.",
        "FromFrontEnd::filter_view" => "The user typed into a list View to filter it.",
        "FromFrontEnd::closed" => "Answers `close`: the window is gone and focus is back where the user was.",
        "FromFrontEnd::open" => "Asks a menu to open, as `kotodama open` does.",
        "FromFrontEnd::toggle" => "Asks a menu to open or close, as `kotodama toggle` does.",
        "FromFrontEnd::undo" => "Undoes the change `offer_undo` offered.",
        "FromFrontEnd::shown" => "The menu became visible.",
        "FromFrontEnd::complete_path" => "Asks for the paths that complete what is typed into a form's file field.",
        "ToFrontEnd::results" => "The whole ranked list for the query, best first, and the app it is in.",
        "ToFrontEnd::status" => "The Extensions that are down, and the problems the user can fix.",
        "ToFrontEnd::close" => "Close the menu, then answer `closed`.",
        "ToFrontEnd::action_failed" => "An Action failed; show `message` and stay open.",
        "ToFrontEnd::push_view" => "Push an Extension's View, with the Contributed actions on its Items.",
        "ToFrontEnd::update_view" => "Replace the open View with the same id.",
        "ToFrontEnd::replace_view" => "Pop one View and push another in one step.",
        "ToFrontEnd::filtered_view" => "The Items of a list View that match its filter, best first.",
        "ToFrontEnd::pop_view" => "Take a View away.",
        "ToFrontEnd::job_progress" => "A job's state, for its progress View.",
        "ToFrontEnd::open" => "Open the menu, as another Front end asked.",
        "ToFrontEnd::toggle" => "Open the menu if it is closed, else close it.",
        "ToFrontEnd::offer_undo" => "The History changed and can be undone: show `title` on the Toast.",
        "ToFrontEnd::withdraw_undo" => "The change can't be undone any more: hide its offer.",
        "ToFrontEnd::set_query" => "Replace the search text, as on entering an app.",
        "ToFrontEnd::reached" => "How many menus an `open` or `toggle` reached.",
        "ToFrontEnd::path_completions" => {
            "The paths that complete the text typed into a file field, best first: the entries of the directory \
             typed, or with nothing typed the newest files the field takes from the download, documents and \
             desktop directories."
        }
        _ => return None,
    })
}

/// The words for a sample of a message between the Daemon and an Extension.
fn describe_extension_message(label: &str) -> Option<&'static str> {
    Some(match label {
        "ToExtension::config" => {
            "The Extension's `[extensions.<name>]` table from the user's config, the first message after the \
             handshake and again whenever the config changes. Never read config files yourself."
        }
        "ToExtension::live_query" => {
            "A query for an Extension whose Manifest asks for Live queries, once it has been still for the \
             Manifest's `debounce_ms`. `query` is the text without the `prefix` that routed it here; `prefix` is `null` \
             for a trigger or a fallback. Answer with `live_results` naming the same `id`."
        }
        "ToExtension::cancel_live_query" => {
            "The query changed or the menu closed: results for Live query `id` are no longer wanted. Stop work \
             on it; results sent for it anyway are dropped."
        }
        "ToExtension::run_action" => {
            "Runs one of the Extension's Actions on an Item: its own, or another Extension's for a Contributed \
             action, of which `item` is all it sees. `arguments` holds the Arguments by name; one of kind `file` or \
             `folder` is an absolute path. Answer with `action_done` and the same `invocation`."
        }
        "ToExtension::submit_form" => {
            "The user submitted a form View the Extension pushed: the values by field id, a `file` field's as an \
             absolute path."
        }
        "ToExtension::view_popped" => {
            "The user left one of the Extension's Views, as with Esc. Closing the menu never sends it, so it may \
             mean \"cancel\"."
        }
        "ToExtension::filter_view" => {
            "The whole filter typed into a list View that has `filters_itself`, empty once cleared. Answer with \
             an `update_view` holding the Items that match."
        }
        "ToExtension::action_catalog" => {
            "Every Action on offer, without any Item, for an Extension whose Manifest has \
             `action_catalog = true`, after it sent `request_action_catalog` and again whenever the catalog \
             changes."
        }
        "ToExtension::rejected" => {
            "The Daemon refused something the Extension sent, and says why in words for its author: a message \
             that doesn't decode, an Item of another Extension's kind, a request it may not make. What was \
             refused is dropped and the connection goes on. Nothing answers it; log it to stderr."
        }
        "FromExtension::add_items" => {
            "Indexes Items. An Item whose id was sent before replaces the old one, which is how an Item is \
             updated."
        }
        "FromExtension::remove_items" => "Takes the Items with these ids out of the index.",
        "FromExtension::live_results" => {
            "Results for Live query `live_query`, in as many batches as you like; `done` says none follow. \
             `score` from 0 to 1 says how well each matches. `answer` marks the one Result that answers the \
             query outright, such as a sum, which takes the top row; only an Extension whose `debounce_ms` is 0 \
             may send one."
        }
        "FromExtension::action_done" => {
            "Answers `run_action` with the same `invocation` and how it finished. Send it for every run, soon: \
             the menu waits for it."
        }
        "FromExtension::push_view" => "Puts a View on top of the menu.",
        "FromExtension::update_view" => "Replaces the open View with the same id.",
        "FromExtension::pop_view" => "Takes the View with this id away.",
        "FromExtension::replace_view" => {
            "Pops the open View `view` and pushes `with` in one step, as a form gives way to its progress, so \
             the search never shows in between. Pushes `with` alone if `view` isn't open."
        }
        "FromExtension::job_progress" => {
            "Reports a job's whole state on each change: `progress` from 0 to 1 if known, and a `message`. A job \
             outlives the menu, and the Daemon sends a desktop notification when it ends."
        }
        "FromExtension::request_action_catalog" => {
            "Asks for `action_catalog`; allowed only with `action_catalog = true` in the Manifest."
        }
        "FromExtension::withdraw_pick" => {
            "Withdraws the pick that running an Action made, as when the app it launched closed right away: \
             the History becomes as if the user never picked it. Only for the Extension's own run, within a \
             minute of `run_action`."
        }
        "FromExtension::status" => {
            "Whether the Extension can do anything now, sent whenever that changes. The search shows its app's \
             entry Item only while it can. `note`, shown on the app's intro card, says why it can't, or what \
             the user should know. Send it again after every start."
        }
        "FromExtension::target_action" => {
            "Offers the Contributed action `action`, declared with `targeted = true` in the Manifest, on every \
             other Extension's Result whose Identity is in `identities`. Each one replaces the set sent before for \
             that Action, so send the whole set whenever it changes; an empty one withdraws it. Send it again \
             after every start."
        }
        _ => return None,
    })
}

#[test]
fn the_protocol_reference_is_written_from_this_versions_pinned_samples() {
    let layout = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("tests/layouts/{PROTOCOL_VERSION}.txt")),
    )
    .expect("read the pinned layout");
    let actual = render(&layout).unwrap_or_else(|err| panic!("{err}"));
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/protocol.md");
    let pinned = std::fs::read_to_string(&path).unwrap_or_default();
    if pinned != actual {
        assert!(
            std::env::var_os(BLESS).is_some(),
            "docs/protocol.md doesn't describe protocol {PROTOCOL_VERSION}'s pinned samples. Rerun this test \
             with {BLESS}=1 to rewrite it."
        );
        std::fs::write(&path, &actual).expect("write docs/protocol.md");
    }
}

/// The document for a pinned layout, or which samples it can't place.
fn render(layout: &str) -> Result<String, String> {
    let samples = json_samples(layout);
    let mut undescribed = Vec::new();
    let mut unplaced = Vec::new();
    for (label, _) in &samples {
        if describe(label).is_none() {
            undescribed.push(*label);
        }
        let group = label.split_once("::").map_or(*label, |(group, _)| group);
        if !SECTIONS.iter().any(|(name, ..)| *name == group) {
            unplaced.push(*label);
        }
    }
    if !undescribed.is_empty() {
        return Err(format!(
            "these samples have no description in tests/protocol_doc.rs's `describe`: {undescribed:?}"
        ));
    }
    if !unplaced.is_empty() {
        return Err(format!(
            "these samples belong to no section in tests/protocol_doc.rs's `SECTIONS`: {unplaced:?}"
        ));
    }

    let mut out = String::new();
    write_intro(&mut out);
    for (group, heading, intro) in SECTIONS {
        writeln!(out, "\n## {heading}: `{group}`\n\n{intro}").unwrap();
        for (label, json) in samples
            .iter()
            .filter(|(label, _)| label.split_once("::").is_some_and(|(owner, _)| owner == group))
        {
            let name = label.split_once("::").map_or(*label, |(_, name)| name);
            let description = describe(label).unwrap_or_default();
            writeln!(out, "\n### `{name}`\n\n{description}\n\n```json\n{}\n```", pretty(json)).unwrap();
        }
    }
    Ok(out)
}

fn write_intro(out: &mut String) {
    writeln!(
        out,
        "<!-- Written by crates/kotodama-protocol/tests/protocol_doc.rs from tests/layouts/{PROTOCOL_VERSION}.txt; \
         don't edit. Rerun it with {BLESS}=1 after the layout changes. -->\n\
         \n\
         # The Kotodama protocol {PROTOCOL_VERSION}, message by message\n\
         \n\
         Every message of protocol {PROTOCOL_VERSION}, as JSON, with a sample of each. The samples are the ones \
         pinned for this version, so they are exactly what the Daemon sends and reads. Each sets every field, \
         even the optional ones. [extensions.md](extensions.md) is the guide to writing an Extension; this is \
         the reference beside it.\n\
         \n\
         - A message is an object with one key, its name, holding its fields: `{{\"remove_items\": {{\"ids\": \
           [\"a\"]}}}}`. A message without fields is its name as a string: `\"request_action_catalog\"`.\n\
         - Each message travels in one frame: its length in bytes as decimal digits, a newline, then the \
           message.\n\
         - The same messages in Protocol Buffers are in \
           [`kotodama.proto`](../crates/kotodama-protocol/kotodama.proto), MessagePack has the JSON's shape, and \
           postcard is for Rust peers on the Daemon's own version."
    )
    .unwrap();
}

/// The JSON samples of a layout, in its order, by label.
fn json_samples(layout: &str) -> Vec<(&str, &str)> {
    layout
        .lines()
        .skip(1)
        .filter_map(|line| {
            let (label, rest) = line.split_once(' ')?;
            let json = rest.strip_prefix("json ")?;
            Some((label, json))
        })
        .collect()
}

/// Indents compact JSON two spaces a level, keeping its keys in order. An
/// empty object or array stays on one line.
fn pretty(json: &str) -> String {
    let mut out = String::new();
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    let mut chars = json.chars().peekable();
    while let Some(c) = chars.next() {
        if in_string {
            out.push(c);
            match (escaped, c) {
                (true, _) => escaped = false,
                (false, '\\') => escaped = true,
                (false, '"') => in_string = false,
                _ => {}
            }
            continue;
        }
        match c {
            '"' => {
                in_string = true;
                out.push(c);
            }
            '{' | '[' if matches!(chars.peek(), Some('}' | ']')) => {
                out.push(c);
                out.push(chars.next().unwrap());
            }
            '{' | '[' => {
                depth += 1;
                out.push(c);
                newline(&mut out, depth);
            }
            '}' | ']' => {
                depth -= 1;
                newline(&mut out, depth);
                out.push(c);
            }
            ',' => {
                out.push(c);
                newline(&mut out, depth);
            }
            ':' => out.push_str(": "),
            _ => out.push(c),
        }
    }
    out
}

fn newline(out: &mut String, depth: usize) {
    out.push('\n');
    out.push_str(&"  ".repeat(depth));
}

#[test]
fn pretty_json_keeps_key_order_and_leaves_strings_alone() {
    assert_eq!(
        pretty(r#"{"b":[1,{}],"a":"x,{\"y\"}:"}"#),
        "{\n  \"b\": [\n    1,\n    {}\n  ],\n  \"a\": \"x,{\\\"y\\\"}:\"\n}"
    );
}

#[test]
fn a_sample_without_a_description_fails_with_its_label() {
    let layout = "protocol version 1.0\nToExtension::brand_new json \"brand_new\"\n";
    let err = render(layout).unwrap_err();
    assert!(err.contains("ToExtension::brand_new"), "{err}");
}

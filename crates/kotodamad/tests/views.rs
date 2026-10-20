//! List Views and the View stack as the Daemon serves them: Contributed
//! actions on a list View's Items, filtering a list View as the user types,
//! replacing one View with the next, and the Views a Front end that connects
//! is shown.

mod common;

use std::collections::BTreeMap;
use std::sync::mpsc;

use common::*;
use kotodama_protocol::*;
use kotodamad::daemon::{Event, FrontEndId};

fn file(id: &str, title: &str) -> Item {
    item(
        id,
        title,
        ItemKind::File {
            path: format!("/files/{id}").into(),
        },
    )
}

fn list(id: &str, items: Vec<Item>) -> View {
    View {
        id: id.into(),
        title: id.into(),
        content: ViewContent::List {
            items,
            filters_itself: false,
        },
    }
}

fn detail(id: &str) -> View {
    View {
        id: id.into(),
        title: id.into(),
        content: ViewContent::Detail {
            markdown: id.into(),
            metadata: vec![],
        },
    }
}

fn filter(h: &mut Harness, ui: &FrontEnd, view: &str, text: &str) {
    h.front_end_sends(
        ui,
        FromFrontEnd::FilterView {
            extension: "files".into(),
            view: view.into(),
            text: text.into(),
        },
    );
}

/// What a Front end was sent, but Results.
fn sent(ui: &FrontEnd) -> Vec<ToFrontEnd> {
    ui.drain()
        .into_iter()
        .filter(|message| !matches!(message, ToFrontEnd::Results { .. }))
        .collect()
}

/// The positions and highlights of each match a [`ToFrontEnd::FilteredView`]
/// lists, with the text it answers.
fn matches(message: &ToFrontEnd) -> (&str, Vec<(u32, Vec<u32>)>) {
    let ToFrontEnd::FilteredView { text, matches, .. } = message else {
        panic!("expected a filtered View, got {message:?}");
    };
    let matches = matches
        .iter()
        .map(|found| (found.item, found.highlights.clone()))
        .collect();
    (text, matches)
}

fn offered(extension: &str, id: &str) -> OfferedAction {
    OfferedAction {
        extension: extension.into(),
        action: action(id),
    }
}

/// `files` pushes list Views; `pinner` contributes to `file` Items, `git` to
/// `project` Items, and `files` to its own `file` Items too.
fn contributors() -> Harness {
    Harness::new(vec![
        manifest(
            r#"
            name = "files"
            command = ["x"]
            [[contributed_actions]]
            kind = "file"
            id = "own-contribution"
            title = "own-contribution"
            "#,
        ),
        manifest(
            r#"
            name = "pinner"
            command = ["x"]
            [[contributed_actions]]
            kind = "file"
            id = "pin"
            title = "pin"
            "#,
        ),
        manifest(
            r#"
            name = "git"
            command = ["x"]
            [[contributed_actions]]
            kind = "project"
            id = "log"
            title = "log"
            "#,
        ),
    ])
}

#[test]
fn the_items_of_a_list_view_are_offered_contributed_actions_by_item_kind() {
    let mut h = contributors();
    let files = h.up(0);
    let pinner = h.up(1);
    h.up(2);
    let ui = h.front_end();
    let view = list(
        "recent",
        vec![
            file("notes", "notes.md"),
            item(
                "site",
                "Site",
                ItemKind::Url {
                    url: "https://example.org".into(),
                },
            ),
        ],
    );
    h.send(&files, FromExtension::PushView(view.clone()));

    assert_eq!(
        sent(&ui),
        [ToFrontEnd::PushView {
            extension: "files".into(),
            view,
            contributed: BTreeMap::from([("file".into(), vec![offered("pinner", "pin")])]),
        }],
        "by the kinds the list has, never the owner's own contribution"
    );

    pinner.drain();
    run(&mut h, &ui, "files", "notes", "pinner", "pin");
    let ran = pinner.drain();
    assert!(
        matches!(&ran[..], [ToExtension::RunAction { action, item, .. }]
            if action == "pin" && item.id == "notes"),
        "{ran:?}"
    );
}

/// `thread`, `Read later`, `Downloads` and `readme.md`, in that order.
fn reading() -> View {
    list(
        "recent",
        vec![
            file("thread", "thread"),
            file("later", "Read later"),
            file("downloads", "Downloads"),
            file("readme", "readme.md"),
        ],
    )
}

#[test]
fn typing_filters_a_list_view_best_match_first_and_in_the_extensions_order_within_a_tier() {
    let mut h = Harness::new(vec![plain("files")]);
    let files = h.up(0);
    let ui = h.front_end();
    h.send(&files, FromExtension::PushView(reading()));
    ui.drain();

    filter(&mut h, &ui, "recent", "read");
    let sent = sent(&ui);
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert_eq!(
        matches(&sent[0]),
        (
            "read",
            vec![
                (1, vec![0, 1, 2, 3]),
                (3, vec![0, 1, 2, 3]),
                (0, vec![2, 3, 4, 5]),
            ]
        ),
        "word starts in list order, then the match inside a word; Downloads doesn't match"
    );
    assert!(
        files.drain().iter().all(|message| !matches!(
            message,
            ToExtension::FilterView { .. } | ToExtension::LiveQuery { .. }
        )),
        "the Extension isn't asked"
    );
}

#[test]
fn typing_on_or_back_matches_what_the_same_text_typed_afresh_does() {
    let typing = |texts: &[&str]| {
        let mut h = Harness::new(vec![plain("files")]);
        let files = h.up(0);
        let ui = h.front_end();
        h.send(&files, FromExtension::PushView(reading()));
        ui.drain();
        let mut last = Vec::new();
        for text in texts {
            filter(&mut h, &ui, "recent", text);
            last = matches(&sent(&ui)[0]).1;
        }
        last
    };
    let typed = [
        "r", "re", "rea", "read", "rea", "read", "readm", "read l", "rd",
    ];
    for n in 1..=typed.len() {
        assert_eq!(
            typing(&typed[..n]),
            typing(&typed[n - 1..n]),
            "after typing {:?}",
            &typed[..n]
        );
    }
}

#[test]
fn typing_on_narrows_a_list_views_matches_as_it_narrows_the_searchs() {
    // `reading b` has no Tier for `Reading Abacus`, where a one-letter word
    // only matches at a word start, but `reading ba` does, inside a word.
    let items = vec![
        file("abacus", "Reading Abacus"),
        file("list", "Reading list"),
        file("thread", "thread"),
        file("downloads", "Downloads"),
        file("readme", "readme.md"),
    ];
    let typing = |texts: &[&str]| {
        let mut h = Harness::new(vec![plain("files")]);
        let files = h.up(0);
        let ui = h.front_end();
        h.add(&files, items.clone());
        h.send(
            &files,
            FromExtension::PushView(list("recent", items.clone())),
        );
        ui.drain();
        let (mut searched, mut filtered) = (Vec::new(), Vec::new());
        for text in texts {
            searched = h
                .query(&ui, text)
                .into_iter()
                .map(|row| row.item.id)
                .collect();
            filter(&mut h, &ui, "recent", text);
            filtered = matches(&sent(&ui)[0])
                .1
                .into_iter()
                .map(|(n, _)| items[n as usize].id.clone())
                .collect();
        }
        searched.sort();
        filtered.sort();
        (searched, filtered)
    };
    let typed = [
        "r",
        "re",
        "reading",
        "reading ",
        "reading b",
        "reading ba",
        "reading b",
        "reading bac",
    ];

    for n in 1..=typed.len() {
        let (searched, filtered) = typing(&typed[..n]);
        assert_eq!(filtered, searched, "after typing {:?}", &typed[..n]);
        assert_eq!(
            (searched, filtered),
            typing(&typed[n - 1..n]),
            "after typing {:?}",
            &typed[..n]
        );
    }
}

#[test]
fn only_the_first_256_matches_of_a_long_list_have_highlights() {
    let mut h = Harness::new(vec![plain("files")]);
    let files = h.up(0);
    let ui = h.front_end();
    let notes = (0..300)
        .map(|n| file(&n.to_string(), &format!("note {n}")))
        .collect();
    h.send(&files, FromExtension::PushView(list("notes", notes)));
    ui.drain();

    filter(&mut h, &ui, "notes", "note");
    let (_, found) = matches(&sent(&ui)[0]);
    assert_eq!(found.len(), 300);
    assert_eq!(found[255].1, [0, 1, 2, 3]);
    assert_eq!(found[256].1, Vec::<u32>::new());
}

#[test]
fn a_cleared_filter_sends_nothing_since_every_item_shows_in_order() {
    let mut h = Harness::new(vec![plain("files")]);
    let files = h.up(0);
    let ui = h.front_end();
    h.send(&files, FromExtension::PushView(reading()));
    filter(&mut h, &ui, "recent", "read");
    ui.drain();

    filter(&mut h, &ui, "recent", "");
    assert_eq!(sent(&ui), []);
}

#[test]
fn a_list_view_updated_while_filtered_is_filtered_again_in_the_menus_filtering_it() {
    let mut h = Harness::new(vec![plain("files")]);
    let files = h.up(0);
    let filtering = h.front_end();
    let other = h.front_end();
    h.send(&files, FromExtension::PushView(reading()));
    filter(&mut h, &filtering, "recent", "read");
    filtering.drain();
    other.drain();

    let updated = list(
        "recent",
        vec![file("readme", "readme.md"), file("downloads", "Downloads")],
    );
    h.send(&files, FromExtension::UpdateView(updated.clone()));
    let update = ToFrontEnd::UpdateView {
        extension: "files".into(),
        view: updated,
        contributed: BTreeMap::new(),
    };
    let sent_filtering = sent(&filtering);
    assert_eq!(sent_filtering.len(), 2, "{sent_filtering:?}");
    assert_eq!(sent_filtering[0], update);
    assert_eq!(
        matches(&sent_filtering[1]),
        ("read", vec![(0, vec![0, 1, 2, 3])])
    );
    assert_eq!(sent(&other), [update], "this one isn't filtering");
}

#[test]
fn a_list_view_that_filters_itself_is_sent_what_is_typed() {
    let mut h = Harness::new(vec![plain("files")]);
    let files = h.up(0);
    let ui = h.front_end();
    let mut view = reading();
    view.content = ViewContent::List {
        items: vec![],
        filters_itself: true,
    };
    h.send(&files, FromExtension::PushView(view));
    ui.drain();
    files.drain();

    filter(&mut h, &ui, "recent", "rea");
    assert_eq!(
        files.drain(),
        [ToExtension::FilterView {
            view: "recent".into(),
            text: "rea".into(),
        }]
    );
    assert_eq!(sent(&ui), [], "the Extension answers with an update");
}

#[test]
fn a_front_end_that_connects_clears_what_a_list_that_filters_itself_was_filtered_by() {
    let mut h = Harness::new(vec![plain("files")]);
    let files = h.up(0);
    let ui = h.front_end();
    let mut view = reading();
    view.content = ViewContent::List {
        items: vec![],
        filters_itself: true,
    };
    h.send(&files, FromExtension::PushView(view));
    filter(&mut h, &ui, "recent", "rea");
    files.drain();

    // Its filter box starts empty, so the Extension lists every Item again.
    connects(&mut h);
    assert_eq!(
        files.drain(),
        [ToExtension::FilterView {
            view: "recent".into(),
            text: String::new(),
        }]
    );
}

#[test]
fn a_filtered_list_updated_into_another_kind_of_view_is_not_filtered_any_more() {
    let mut h = Harness::new(vec![plain("files")]);
    let files = h.up(0);
    let ui = h.front_end();
    h.send(&files, FromExtension::PushView(reading()));
    filter(&mut h, &ui, "recent", "read");
    ui.drain();

    h.send(&files, FromExtension::UpdateView(detail("recent")));
    let sent = sent(&ui);
    assert!(
        matches!(&sent[..], [ToFrontEnd::UpdateView { .. }]),
        "no matches follow: {sent:?}"
    );
    // Nor when it is a list again.
    h.send(&files, FromExtension::UpdateView(reading()));
    let sent = self::sent(&ui);
    assert!(
        matches!(&sent[..], [ToFrontEnd::UpdateView { .. }]),
        "{sent:?}"
    );
}

#[test]
fn a_list_views_contributed_actions_follow_the_extensions_that_start_and_stop() {
    let mut h = contributors();
    let files = h.up(0);
    let ui = h.front_end();
    let view = list("recent", vec![file("notes", "notes.md")]);
    h.send(&files, FromExtension::PushView(view.clone()));
    ui.drain();

    let pinner = h.up(1);
    assert!(
        sent(&ui).contains(&ToFrontEnd::UpdateView {
            extension: "files".into(),
            view: view.clone(),
            contributed: BTreeMap::from([("file".into(), vec![offered("pinner", "pin")])]),
        }),
        "the View's Items now offer pinner's Action"
    );

    // An Extension that contributes nothing to it changes nothing.
    h.up(2);
    assert!(
        !sent(&ui)
            .iter()
            .any(|message| matches!(message, ToFrontEnd::UpdateView { .. }))
    );

    h.down(&pinner);
    assert!(sent(&ui).contains(&ToFrontEnd::UpdateView {
        extension: "files".into(),
        view,
        contributed: BTreeMap::new(),
    }));
}

#[test]
fn a_filter_for_a_view_that_isnt_open_is_ignored() {
    let mut h = Harness::new(vec![plain("files")]);
    h.up(0);
    let ui = h.front_end();
    filter(&mut h, &ui, "recent", "read");
    assert_eq!(sent(&ui), []);
}

#[test]
fn replacing_a_view_pops_it_and_pushes_the_next_in_one_message() {
    let mut h = Harness::new(vec![plain("files")]);
    let files = h.up(0);
    let ui = h.front_end();
    h.send(&files, FromExtension::PushView(detail("step-1")));
    ui.drain();

    h.send(
        &files,
        FromExtension::ReplaceView {
            view: "step-1".into(),
            with: reading(),
        },
    );
    assert_eq!(
        sent(&ui),
        [ToFrontEnd::ReplaceView {
            extension: "files".into(),
            view: "step-1".into(),
            with: reading(),
            contributed: BTreeMap::new(),
        }]
    );

    files.drain();
    run(&mut h, &ui, "files", "readme", "files", "open");
    assert!(
        files.drain().iter().any(
            |message| matches!(message, ToExtension::RunAction { item, .. } if item.id == "readme")
        ),
        "the new list's Items run"
    );
}

/// A Front end that connects now, with what the Daemon sent it on connecting
/// still unread.
fn connects(h: &mut Harness) -> FrontEnd {
    let id = FrontEndId(1000 + h.next_front_end);
    h.next_front_end += 1;
    let (tx, rx) = mpsc::sync_channel(OUTBOX);
    h.event(Event::FrontEndUp {
        front_end: id,
        sender: tx,
        disconnect: Box::new(|| {}),
    });
    FrontEnd {
        id,
        rx,
        closed: Default::default(),
        shown: Default::default(),
    }
}

/// The Views pushed, by id, in order.
fn pushed(sent: &[ToFrontEnd]) -> Vec<&str> {
    sent.iter()
        .filter_map(|message| match message {
            ToFrontEnd::PushView { view, .. } => Some(view.id.as_str()),
            _ => None,
        })
        .collect()
}

#[test]
fn a_front_end_that_connects_is_shown_the_views_still_open_bottom_first() {
    let mut h = contributors();
    let files = h.up(0);
    h.up(1);
    let ui = h.front_end();
    h.send(&files, FromExtension::PushView(detail("a")));
    h.send(&files, FromExtension::PushView(reading()));
    h.send(&files, FromExtension::PushView(detail("b")));
    h.send(&files, FromExtension::PushView(detail("c")));
    // Pushed again, it goes back on top, as in the menus.
    h.send(&files, FromExtension::PushView(detail("a")));
    h.send(&files, FromExtension::PopView { view: "b".into() });
    h.send(
        &files,
        FromExtension::ReplaceView {
            view: "c".into(),
            with: detail("d"),
        },
    );
    h.front_end_sends(
        &ui,
        FromFrontEnd::ViewPopped {
            extension: "files".into(),
            view: "d".into(),
        },
    );

    let sent = connects(&mut h).drain();
    assert_eq!(pushed(&sent), ["recent", "a"]);
    assert!(
        sent.iter().any(|message| matches!(message,
            ToFrontEnd::PushView { view, contributed, .. }
                if view.id == "recent" && contributed.contains_key("file"))),
        "a list View with its Contributed actions: {sent:?}"
    );
}

#[test]
fn a_front_end_that_connects_is_not_shown_views_of_an_extension_that_stopped() {
    let mut h = Harness::new(vec![plain("files")]);
    let files = h.up(0);
    h.send(&files, FromExtension::PushView(detail("a")));
    h.down(&files);

    assert_eq!(pushed(&connects(&mut h).drain()), Vec::<&str>::new());
}

use kotodama_protocol::Action;
use kotodama_protocol::Item;
use kotodama_protocol::ItemKind;

use super::*;
use crate::clock::DAY_SECS;
use crate::daemon::Source;
use crate::history::Frecent;
use crate::history::History;
use crate::most_used::Counts;
use crate::most_used::Picked;

fn item(id: &str) -> Item {
    Item {
        id: id.into(),
        kind: ItemKind::Url {
            url: format!("https://example.org/{id}"),
        },
        identity: None,
        title: id.into(),
        subtitle: None,
        icon: None,
        keyword: None,
        keywords: vec![],
        hidden: false,
        app_only: false,
        weak_keywords: Vec::new(),
        yields: false,
        actions: vec![Action {
            id: "open".into(),
            title: "Open".into(),
            shortcut: None,
            arguments: vec![],
            confirm: false,
            keywords: Vec::new(),
            role: kotodama_protocol::ActionRole::Normal,
        }],
    }
}

/// Each version built, as where each Segment in it goes and the change
/// it holds.
fn versions(built: &[Indexed]) -> Vec<(usize, u64)> {
    built
        .iter()
        .flat_map(|indexed| indexed.segments.iter().map(|(at, _)| (*at, indexed.change)))
        .collect()
}

/// The Segment at `at` in what was built last.
fn segment(built: &[Indexed], at: usize) -> &Segment {
    built
        .iter()
        .rev()
        .find_map(|indexed| indexed.segments.iter().find(|(n, _)| *n == at))
        .map(|(_, segment)| &**segment)
        .unwrap()
}

/// Runs the thread's work over what is queued already.
fn built(jobs: Vec<Job>) -> Vec<Indexed> {
    let (sender, queue) = mpsc::channel();
    for job in jobs {
        sender.send(job).unwrap();
    }
    drop(sender);
    let mut built = Vec::new();
    work(&queue, None, |event| {
        if let Event::Indexed(indexed) = event {
            built.push(indexed);
        }
        true
    });
    built
}

fn edit(extension: usize, change: u64, edit: Edit) -> Job {
    Job::Edit {
        extension: ExtensionId(extension),
        change,
        edit,
    }
}

#[test]
fn changes_waiting_together_make_one_version_of_each_segment() {
    let built = built(vec![
        edit(0, 1, Edit::Add(vec![item("a")])),
        edit(1, 2, Edit::Add(vec![item("b")])),
        edit(0, 3, Edit::Add(vec![item("c")])),
        edit(0, 4, Edit::Remove(vec!["a".into()])),
    ]);
    assert_eq!(versions(&built), [(0, 4), (2, 2)]);
    let first = &built[0].segments[0].1;
    assert_eq!(first.find("a"), None);
    assert!(first.find("c").is_some());
}

#[test]
fn a_row_emptied_in_a_version_is_not_taken_in_the_same_one() {
    let built = built(vec![
        edit(0, 1, Edit::Add(vec![item("a"), item("b")])),
        edit(0, 2, Edit::Remove(vec!["a".into()])),
        edit(0, 3, Edit::Add(vec![item("c")])),
    ]);
    let segment = &built[0].segments[0].1;
    assert_eq!(segment.find("b"), Some(1));
    assert_eq!(segment.find("c"), Some(2));
}

/// Runs the thread's work over what is queued already, and gives what it
/// sent: for each event, the slots of the Items read along with it.
fn sent(jobs: Vec<Job>) -> Vec<Vec<Slot>> {
    let (sender, queue) = mpsc::channel();
    for job in jobs {
        sender.send(job).unwrap();
    }
    drop(sender);
    let mut sent = Vec::new();
    work(&queue, None, |event| {
        let reads = match event {
            Event::Indexed(indexed) => indexed.reads,
            Event::Kept(kept) => kept.reads,
            _ => unreachable!(),
        };
        sent.push(reads.iter().map(|read| read.slot).collect());
        true
    });
    sent
}

fn slot(row: Row) -> Slot {
    Slot { segment: 0, row }
}

#[test]
fn items_named_by_a_keyword_or_an_alias_are_read_with_their_version_once() {
    let mut keyed = item("b");
    keyed.keyword = Some("bb".into());
    let sent = sent(vec![
        Job::Pin(vec![(ExtensionId(0), "c".into())]),
        edit(0, 1, Edit::Add(vec![item("a"), keyed, item("c")])),
        edit(0, 2, Edit::Add(vec![item("d")])),
    ]);
    // The version, then the Aliases' Items, read already.
    assert_eq!(sent, [vec![slot(1), slot(2)], vec![]]);
}

const EIGHT: When = When {
    at: 10 * DAY_SECS,
    time_of_day: 8 * 3600,
    day_of_week: 6,
};

/// A plan at eight o'clock, of `rows` picked at eight `picks` times
/// each and at no other time, with the Empty query showing `wanted`.
fn plan(rows: &[(Row, u32)], learned: &[Row], capacity: usize, wanted: usize) -> Plan {
    let mut history = History::default();
    let mut picked = Vec::new();
    for &(row, picks) in rows {
        let id = row.to_string();
        for _ in 0..picks {
            history.touch("apps", &id, EIGHT);
        }
        picked.push(*history.picks("apps", &id).unwrap());
    }
    let half_life = history.settings().half_life_secs;
    Plan {
        usage: Usage {
            picked: Arc::new(Picked {
                slots: rows.iter().map(|&(row, _)| (slot(row), Counts::Both)).collect(),
                groups: vec![None; rows.len()],
                picks: Frecent::new(&picked, half_life),
            }),
            others: Vec::new(),
            wanted,
            left_out: Vec::new(),
        },
        learned: learned.iter().map(|&row| slot(row)).collect(),
        when: EIGHT,
        capacity,
        versions: (0, 0, 0),
    }
}

fn slots(rows: &[Row]) -> Vec<Slot> {
    rows.iter().map(|&row| slot(row)).collect()
}

#[test]
fn the_hot_items_are_planned_and_read_here() {
    let sent = sent(vec![
        edit(0, 1, Edit::Add(vec![item("a"), item("b")])),
        Job::Plan(plan(&[(1, 1)], &[], 8, 0)),
    ]);
    assert_eq!(sent, [vec![], vec![slot(1)]]);
}

#[test]
fn learned_choices_take_a_quarter_of_the_hot_items_before_the_habit() {
    let habit: Vec<(Row, u32)> = (0..10).map(|row| (row, 10 - row)).collect();

    let (hot, _) = plan(&habit, &[20, 21, 22], 8, 0).run();

    assert_eq!(hot, slots(&[0, 1, 2, 3, 4, 5, 20, 21]));
}

#[test]
fn learned_choices_take_the_room_the_habit_leaves() {
    let (hot, _) = plan(&[(0, 1)], &[20, 21, 22, 23], 4, 0).run();

    assert_eq!(hot, slots(&[0, 20, 21, 22]));
}

#[test]
fn the_items_the_empty_query_shows_are_hot_before_learned_choices() {
    let habit = [(0, 5), (1, 4), (2, 3), (3, 2), (4, 1)];

    let (hot, ahead) = plan(&habit, &[20], 4, 4).run();

    let ranked: Vec<&[Source]> = ahead.ranked.iter().map(|(_, ranked)| &ranked[..]).collect();
    let shown = [0, 1, 2, 3].map(|row| Source::Indexed(slot(row)));
    assert_eq!(ranked, [shown, shown]);
    assert_eq!(hot, slots(&[0, 1, 2, 3]));
}

fn app_only(id: &str) -> Item {
    Item {
        app_only: true,
        ..item(id)
    }
}

#[test]
fn an_extensions_items_go_to_the_segment_their_visibility_names() {
    let built = built(vec![edit(1, 1, Edit::Add(vec![item("a"), app_only("b")]))]);

    assert_eq!(versions(&built), [(2, 1), (3, 1)]);
    assert!(segment(&built, 2).find("a").is_some());
    assert_eq!(segment(&built, 2).find("b"), None);
    assert!(segment(&built, 3).find("b").is_some());
}

#[test]
fn an_item_that_changes_visibility_leaves_its_old_segment() {
    let built = built(vec![
        edit(0, 1, Edit::Add(vec![item("a"), item("b")])),
        edit(0, 2, Edit::Add(vec![app_only("a")])),
    ]);

    assert_eq!(built.len(), 1, "both Segments in one version");
    assert_eq!(segment(&built, 0).find("a"), None);
    assert!(segment(&built, 0).find("b").is_some());
    assert!(segment(&built, 1).find("a").is_some());
}

#[test]
fn a_change_to_one_segment_leaves_the_other_alone() {
    let built = built(vec![edit(0, 1, Edit::Add(vec![app_only("b")]))]);

    assert_eq!(versions(&built), [(1, 1)]);
}

#[test]
fn a_change_that_touches_nothing_still_makes_a_version() {
    let built = built(vec![edit(0, 1, Edit::Remove(vec!["gone".into()]))]);

    assert_eq!(versions(&built), [(0, 1)]);
}

#[test]
fn a_cleared_extensions_earlier_changes_are_not_sent() {
    let built = built(vec![
        edit(0, 1, Edit::Add(vec![item("a")])),
        Job::Clear(ExtensionId(0)),
        edit(1, 2, Edit::Add(vec![item("b")])),
    ]);
    assert_eq!(built.len(), 1);
    assert_eq!(built[0].extension, ExtensionId(1));
}

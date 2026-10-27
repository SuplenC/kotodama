use super::*;

const HOUR: u32 = 3600;

/// `hour` seconds into day `day`, in UTC. Day 0 is a Monday.
fn when(day: u64, hour: u32) -> When {
    When::utc(1_792_368_000 + day * DAY_SECS + u64::from(hour))
}

/// `hour` seconds into the day of the week `day` (0 for Monday) of week
/// `week`, in UTC.
fn on(week: u64, day: u64, hour: u32) -> When {
    when(week * 7 + day, hour)
}

const MONDAY: u64 = 0;
const WEDNESDAY: u64 = 2;
const FRIDAY: u64 = 4;
const SATURDAY: u64 = 5;
const SUNDAY: u64 = 6;

#[test]
fn the_days_of_the_week_fall_where_the_helpers_say() {
    assert_eq!(when(0, 0).day_of_week, 0, "a Monday");
    assert_eq!(on(0, SUNDAY, 0).day_of_week, 6);
    assert_eq!(on(3, SATURDAY, 0).day_of_week, 5);
}

#[test]
fn a_part_of_the_day_starts_every_three_hours_of_local_time() {
    let nine = when(0, 9 * HOUR);
    assert_eq!(part_of_day(nine), part_of_day(when(0, 11 * HOUR + 3599)));
    assert_ne!(part_of_day(nine), part_of_day(when(0, 12 * HOUR)));
    assert_ne!(part_of_day(nine), part_of_day(when(1, 9 * HOUR)));
    for ahead in [HOUR / 2, 3 * HOUR] {
        let in_another_zone = When {
            time_of_day: nine.time_of_day + ahead,
            ..nine
        };
        assert_ne!(part_of_day(nine), part_of_day(in_another_zone), "{ahead}");
    }
}

#[test]
fn the_next_part_of_the_day_starts_where_this_one_ends() {
    assert_eq!(next_part(when(0, 10 * HOUR)), when(0, 12 * HOUR));
    assert_eq!(next_part(when(0, 9 * HOUR)), when(0, 12 * HOUR));
    assert_eq!(next_part(when(0, 23 * HOUR)), when(1, 0));
    let started = next_part(when(0, 10 * HOUR));
    assert_ne!(part_of_day(started), part_of_day(when(0, 12 * HOUR - 1)));
    assert_eq!(part_of_day(started), part_of_day(when(0, 14 * HOUR)));
}

/// 2,000 Results, the most the History keeps, picked one to seven
/// times each over two months at all times of day, some for a query.
fn picked_2000() -> History {
    let mut history = History::default();
    for n in 0..2000u64 {
        for k in 0..=n % 7 {
            let day = (n * 13 + k * 5) % 60;
            let hour = ((n * 7 + k * 11) % 24) as u32 * HOUR + (n % 3600) as u32;
            let id = format!("item-{n}");
            if n % 10 == 0 {
                history.pick("apps", &id, &format!("q{}", n % 50), &[], when(day, hour));
            } else {
                history.touch("apps", &id, when(day, hour));
            }
        }
    }
    history
}

/// Every Result, by its Frecency and Habit at `when`, then the Learned
/// choices for a few queries, each with what it counts.
fn ranked(history: &History, when: When) -> Vec<String> {
    let half_life = history.settings.half_life_secs;
    let mut results: Vec<_> = history
        .picked()
        .map(|(_, id, picks)| {
            let habit = picks.habit(when, half_life);
            let frecency = picks.frecency(when, &history.settings);
            (frecency, habit.near_now, habit.overall, id.to_owned())
        })
        .collect();
    results.sort_by(|a, b| b.0.total_cmp(&a.0).then(b.1.total_cmp(&a.1)).then(a.3.cmp(&b.3)));
    let learned = (0..50)
        .flat_map(|n| history.learned(&format!("q{n}"), when.at))
        .map(|choice| format!("{}/{} {}", choice.extension, choice.id, choice.strength));
    results
        .into_iter()
        .map(|(frecency, near_now, overall, id)| format!("{id} {frecency} {near_now} {overall}"))
        .chain(learned)
        .collect()
}

#[test]
fn a_saved_and_reloaded_history_ranks_identically() {
    let history = picked_2000();
    let json = serde_json::to_string(&history).expect("the History encodes");
    let reloaded: History = serde_json::from_str(&json).expect("the History decodes");
    for at in [when(60, 9 * HOUR), when(75, 22 * HOUR + 1234)] {
        assert_eq!(ranked(&reloaded, at), ranked(&history, at));
    }
}

#[test]
fn a_history_kept_with_full_precision_scores_loads() {
    let history: History = serde_json::from_str(
        r#"{"picks":{"apps":{"firefox":{"score":481.8121234736919,"at":1792368000}}},
            "learned":{"fi":{"choice":["apps","firefox"],"strength":3.141592653589793,
            "contenders":[],"at":1792368000}}}"#,
    )
    .expect("an old History decodes");
    let at = when(0, 0);
    assert_eq!(
        history
            .picks("apps", "firefox")
            .expect("the picks are kept")
            .count(at.at, DAY),
        481.812_133_789_062_5
    );
    assert_eq!(history.learned("fi", at.at)[0].strength, 3.141_592_741_012_573_2);
}

#[test]
fn decayed_together_picks_count_as_each_on_its_own() {
    let history = picked_2000();
    let picks: Vec<Picks> = history.picked().map(|(.., picks)| picks).collect();
    let settings = history.settings;
    let frecent = Frecent::new(&picks, settings.half_life_secs);
    let (mut habits, mut frecency) = (Vec::new(), Vec::new());
    // Also before some of the picks: a clock set back.
    for at in [when(30, 3 * HOUR), when(60, 9 * HOUR), when(400, 17 * HOUR)] {
        frecent.habits(at, &mut habits);
        frecent.frecency(at, &settings, &mut frecency);
        for (n, picks) in picks.iter().enumerate() {
            let alone = picks.habit(at, settings.half_life_secs);
            let close = |a: f64, b: f64| (a - b).abs() <= 1e-12 * b.abs().max(1.0);
            assert!(close(habits[n].overall, alone.overall), "{n}");
            assert!(close(habits[n].near_now, alone.near_now), "{n}");
            assert!(close(frecency[n], picks.frecency(at, &settings)), "{n}");
        }
    }
}

#[test]
fn a_result_takes_under_two_hundred_bytes_however_often_it_is_picked() {
    let mut history = History::default();
    for day in 0..100 {
        for hour in 0..24 {
            history.touch("apps", "firefox", when(day, hour * HOUR + 17 * 60));
        }
    }
    let json = serde_json::to_string(&history).unwrap();
    let entry = json.find("\"firefox\"").map(|at| &json[at..]).unwrap();
    let entry = &entry[..=entry.find('}').unwrap()];
    assert!(entry.len() <= 170, "{entry}");
}

#[test]
fn picks_kept_before_times_of_day_were_count_in_full_at_any_time() {
    let history: History =
        serde_json::from_str(r#"{"picks":{"apps":{"firefox":{"score":2.0,"at":1792368000}}},"learned":{}}"#).unwrap();
    assert_eq!(counts(&history, "firefox", when(0, 3 * HOUR)), 1.0);
    assert_eq!(counts(&history, "firefox", when(0, 15 * HOUR)), 1.0);
}

#[test]
fn picks_kept_before_days_of_the_week_were_count_in_full_on_either_kind_of_day() {
    let history: History = serde_json::from_str(
        r#"{"picks":{"apps":{"firefox":{"score":5.0,"at":1792368000,"parts":[0,0,0,5000,5000,0,0,0]}}}}"#,
    )
    .unwrap();
    assert_eq!(counts(&history, "firefox", on(0, MONDAY, 9 * HOUR)), 1.0);
    assert_eq!(counts(&history, "firefox", on(0, SATURDAY, 9 * HOUR)), 1.0);
    assert_eq!(
        counts(&history, "firefox", on(0, SATURDAY, 21 * HOUR)),
        0.5,
        "the time of day still counts"
    );

    let json = serde_json::to_string(&history).unwrap();
    let reloaded: History = serde_json::from_str(&json).unwrap();
    assert_eq!(reloaded.picks("apps", "firefox"), history.picks("apps", "firefox"));
}

#[test]
fn picks_count_in_full_on_the_kind_of_day_they_are_made_and_less_on_the_other() {
    let mut history = History::default();
    for week in 0..3 {
        for day in MONDAY..=FRIDAY {
            history.touch("apps", "mail", on(week, day, 9 * HOUR));
        }
        for day in [SATURDAY, SUNDAY] {
            history.touch("apps", "games", on(week, day, 9 * HOUR));
        }
    }
    let less = 1.0 - history.settings().day_of_week;
    assert!(less < 1.0, "the default counts the day of the week");
    assert_eq!(counts(&history, "mail", on(3, WEDNESDAY, 9 * HOUR)), 1.0);
    assert_eq!(counts(&history, "mail", on(3, SATURDAY, 9 * HOUR)), less);
    assert_eq!(counts(&history, "games", on(3, SATURDAY, 9 * HOUR)), 1.0);
    assert_eq!(counts(&history, "games", on(3, WEDNESDAY, 9 * HOUR)), less);
}

#[test]
fn a_result_used_daily_counts_in_full_on_both_kinds_of_day() {
    let mut history = History::default();
    for week in 0..3 {
        for day in MONDAY..=SUNDAY {
            history.touch("apps", "browser", on(week, day, 9 * HOUR));
        }
    }
    assert_eq!(counts(&history, "browser", on(3, WEDNESDAY, 9 * HOUR)), 1.0);
    assert_eq!(counts(&history, "browser", on(3, SATURDAY, 9 * HOUR)), 1.0);
}

#[test]
fn picks_fade_from_weekdays_into_weekends_across_midnight() {
    let mut history = History::default();
    for week in 0..3 {
        history.touch("apps", "early", on(week, FRIDAY, 21 * HOUR));
        history.touch("apps", "late", on(week, FRIDAY, 23 * HOUR));
        history.touch("apps", "night", on(week, SATURDAY, HOUR));
    }
    let less = 1.0 - history.settings().day_of_week;
    let close = |a: f64, b: f64| (a - b).abs() <= 1e-9;
    let early = counts(&history, "early", on(3, SATURDAY, 21 * HOUR));
    assert!(close(early, less), "a weekday pick: {early}");
    let late = counts(&history, "late", on(3, SATURDAY, 23 * HOUR));
    assert!(late > less + 0.01 && late < 0.99, "partly a weekend pick: {late}");
    assert_eq!(
        counts(&history, "late", on(3, WEDNESDAY, 23 * HOUR)),
        1.0,
        "mostly a weekday pick"
    );
    let night = counts(&history, "night", on(3, WEDNESDAY, HOUR));
    assert!(night > less + 0.01 && night < 0.99, "partly a weekday pick: {night}");
    assert_eq!(
        counts(&history, "night", on(3, SATURDAY, 2 * HOUR)),
        1.0,
        "mostly a weekend pick"
    );
}

#[test]
fn the_empty_query_counts_picks_near_now_on_this_kind_of_day() {
    let mut history = History::default();
    for week in 0..3 {
        for day in MONDAY..=FRIDAY {
            history.touch("apps", "mail", on(week, day, 9 * HOUR));
        }
    }
    let half_life = history.settings().half_life_secs;
    let picks = history.picks("apps", "mail").unwrap();
    let weekday = picks.habit(on(3, WEDNESDAY, 9 * HOUR), half_life);
    let weekend = picks.habit(on(3, SATURDAY, 9 * HOUR), half_life);
    assert_eq!(weekday.near_now, weekday.overall);
    assert_eq!(weekend.near_now, 0.0);
    assert!(weekend.overall > 0.0);
}

#[test]
fn undoing_a_pick_on_either_kind_of_day_takes_off_that_pick_alone() {
    let mut history = History::default();
    for week in 0..3 {
        history.touch("apps", "mail", on(week, MONDAY, 9 * HOUR));
        history.touch("apps", "mail", on(week, SATURDAY, 9 * HOUR));
    }
    let mut second_only = history.clone();
    let mut withdrawn = history;
    let first_weekday = withdrawn.touch("apps", "mail", on(3, MONDAY, 9 * HOUR));
    withdrawn.touch("apps", "mail", on(3, MONDAY, 10 * HOUR));
    let first_weekend = withdrawn.touch("apps", "mail", on(3, SATURDAY, 9 * HOUR));
    withdrawn.touch("apps", "mail", on(3, SATURDAY, 10 * HOUR));
    withdrawn.undo(first_weekday);
    withdrawn.undo(first_weekend);
    second_only.touch("apps", "mail", on(3, MONDAY, 10 * HOUR));
    second_only.touch("apps", "mail", on(3, SATURDAY, 10 * HOUR));

    let close = |a: f64, b: f64| (a - b).abs() <= 1e-3 * b.abs().max(1.0);
    for at in [
        on(4, MONDAY, 9 * HOUR),
        on(4, SATURDAY, 9 * HOUR),
        on(4, FRIDAY, 23 * HOUR),
    ] {
        let (got, want) = (
            withdrawn.frecency("apps", "mail", at),
            second_only.frecency("apps", "mail", at),
        );
        assert!(close(got, want), "at {at:?}: {got} for {want}");
    }
}

#[test]
fn picks_count_in_full_at_the_time_of_day_they_are_made_and_less_away_from_it() {
    let mut history = History::default();
    for day in 0..5 {
        history.touch("apps", "mail", when(day, 7 * HOUR));
    }
    // Read on the Monday after, a weekday like the picks' days.
    assert_eq!(counts(&history, "mail", when(7, 7 * HOUR)), 1.0);
    let near = counts(&history, "mail", when(7, 9 * HOUR));
    assert!(near > 0.8, "two hours later: {near}");
    assert_eq!(counts(&history, "mail", when(7, 19 * HOUR)), 0.5, "away");
}

#[test]
fn habits_four_hours_apart_each_count_most_at_their_own_time() {
    let mut history = History::default();
    for day in 0..5 {
        history.touch("apps", "mail", when(day, 8 * HOUR));
        history.touch("apps", "news", when(day, 12 * HOUR));
    }
    // Read on the Monday after, a weekday like the picks' days.
    assert!(counts(&history, "news", when(7, 8 * HOUR)) < 0.7);
    assert!(counts(&history, "mail", when(7, 12 * HOUR)) < 0.7);
}

#[test]
fn a_result_picked_hundreds_of_times_a_day_still_learns_a_new_time_of_day() {
    let mut history = History::default();
    for day in 0..60 {
        for minute in 0..300 {
            history.touch("apps", "mail", when(day, 8 * HOUR + minute));
        }
    }
    let before = counts(&history, "mail", when(60, 20 * HOUR));
    for day in 60..74 {
        for minute in 0..300 {
            history.touch("apps", "mail", when(day, 20 * HOUR + minute));
        }
    }
    let after = counts(&history, "mail", when(74, 20 * HOUR));
    assert_eq!(before, 0.5, "never picked in the evening");
    assert!(after > 0.9, "picked as much in the evening lately: {after}");
}

fn key(extension: &str, id: &str) -> ResultKey {
    (extension.to_owned(), id.to_owned())
}

/// Firefox picked through its Identity three times for `fi`, over Files,
/// and Fish three times for `sh`.
fn used() -> History {
    let mut history = History::default();
    let above = [key("apps", "files")];
    for day in 0..3 {
        history.pick(IDENTITIES, "app:firefox", "fi", &above, when(day, 9 * HOUR));
        history.touch("apps", "firefox", when(day, 9 * HOUR));
        history.pick("apps", "fish", "sh", &[], when(day, 10 * HOUR));
    }
    history
}

/// The History as it is kept, which undoing must restore.
fn kept(history: &History) -> serde_json::Value {
    serde_json::to_value(history).unwrap()
}

fn firefox() -> [ResultKey; 2] {
    [key(IDENTITIES, "app:firefox"), key("apps", "firefox")]
}

#[test]
fn forgetting_a_result_drops_its_picks_and_the_learned_choices_for_it() {
    let mut history = used();
    let at = when(3, 0).at;
    assert!(!history.learned("fi", at).is_empty());
    history.forget(&firefox());
    assert!(history.picks(IDENTITIES, "app:firefox").is_none());
    assert!(history.picks("apps", "firefox").is_none());
    assert!(history.learned("fi", at).is_empty());
    assert!(history.picks("apps", "fish").is_some(), "the rest stays");
    assert_eq!(history.learned("sh", at)[0].id, "fish");
}

#[test]
fn a_result_picked_towards_a_learned_choice_is_forgotten_from_it_too() {
    let mut history = History::default();
    history.pick("apps", "fish", "fi", &[], when(0, 9 * HOUR));
    history.forget(&[key("apps", "fish")]);
    assert!(history.is_empty(), "{history:?}");
}

#[test]
fn undoing_a_forget_restores_the_history_exactly() {
    let mut history = used();
    let before = kept(&history);
    let change = history.forget(&firefox());
    history.undo(change);
    assert_eq!(kept(&history), before);
}

#[test]
fn undoing_a_clear_restores_the_history_exactly() {
    let mut history = used();
    let before = kept(&history);
    let change = history.clear();
    assert!(history.is_empty());
    history.undo(change);
    assert_eq!(kept(&history), before);
}

#[test]
fn undoing_a_pick_restores_the_history_exactly() {
    let mut history = used();
    let before = kept(&history);
    let above = [key("apps", "files")];
    let mut change = history.pick(IDENTITIES, "app:firefox", "fi", &above, when(4, 9 * HOUR));
    change.and(history.touch("apps", "firefox", when(4, 9 * HOUR)));
    history.undo(change);
    assert_eq!(kept(&history), before);

    let change = history.pick("apps", "mail", "ma", &[], when(4, 9 * HOUR));
    history.undo(change);
    assert_eq!(kept(&history), before, "a Result picked for the first time");
}

#[test]
fn undoing_a_pick_puts_back_what_the_caps_trimmed_for_it() {
    let mut history = History::default();
    for n in 0..DEFAULT_MAX_RESULTS {
        // A second apart, so one is the weakest.
        history.touch("apps", &n.to_string(), when(0, n as u32));
    }
    let before = kept(&history);
    let change = history.touch("apps", "new", when(1, 9 * HOUR));
    assert_eq!(history.picked().count(), DEFAULT_MAX_RESULTS);
    history.undo(change);
    assert_eq!(kept(&history), before);
}

#[test]
fn undoing_the_first_of_two_picks_takes_off_that_pick_alone() {
    let above = [key("apps", "files")];
    let mut withdrawn = used();
    let first = withdrawn.pick(IDENTITIES, "app:firefox", "fi", &above, when(4, 9 * HOUR));
    withdrawn.pick(IDENTITIES, "app:firefox", "fi", &above, when(4, 21 * HOUR));
    let first_fish = withdrawn.pick("apps", "fish", "sh", &[], when(4, 10 * HOUR));
    withdrawn.pick("apps", "fish", "sh", &[], when(5, 10 * HOUR));
    let first_mail = withdrawn.pick("apps", "mail", "ma", &[], when(4, 9 * HOUR));
    withdrawn.pick("apps", "mail", "ma", &[], when(5, 9 * HOUR));
    for change in [first, first_fish, first_mail] {
        withdrawn.undo(change);
    }

    let mut second_only = used();
    second_only.pick(IDENTITIES, "app:firefox", "fi", &above, when(4, 21 * HOUR));
    second_only.pick("apps", "fish", "sh", &[], when(5, 10 * HOUR));
    second_only.pick("apps", "mail", "ma", &[], when(5, 9 * HOUR));
    let close = |a: f64, b: f64| (a - b).abs() <= 1e-3 * b.abs().max(1.0);
    for at in [when(5, 9 * HOUR), when(5, 21 * HOUR), when(9, 3 * HOUR)] {
        let picked = [(IDENTITIES, "app:firefox"), ("apps", "fish"), ("apps", "mail")];
        for (extension, id) in picked {
            let (got, want) = (
                withdrawn.frecency(extension, id, at),
                second_only.frecency(extension, id, at),
            );
            assert!(close(got, want), "{id} at {at:?}: {got} for {want}");
        }
        for query in ["fi", "sh"] {
            let strengths = |history: &History| -> Vec<(String, f64)> {
                let choices = history.learned(query, at.at);
                choices.iter().map(|c| (c.id.to_owned(), c.strength)).collect()
            };
            let (got, want) = (strengths(&withdrawn), strengths(&second_only));
            assert_eq!(got.len(), want.len(), "{query}: {got:?} for {want:?}");
            for ((got_id, got), (want_id, want)) in got.iter().zip(&want) {
                assert_eq!(got_id, want_id);
                assert!(close(*got, *want), "{query}: {got} for {want}");
            }
        }
    }
    assert_eq!(withdrawn.learned["ma"].contenders, second_only.learned["ma"].contenders);
}

#[test]
fn undoing_a_pick_that_was_forgotten_since_leaves_the_history_alone() {
    let mut history = used();
    let change = history.pick("apps", "mail", "ma", &[], when(4, 9 * HOUR));
    history.forget(&[key("apps", "mail")]);
    let before = kept(&history);
    history.undo(change);
    assert_eq!(kept(&history), before);
}

#[test]
fn undoing_a_pick_that_took_over_brings_back_the_choice_it_replaced() {
    let picks = |history: &mut History, mistaken: bool| {
        history.pick("apps", "fish", "fi", &[], when(5, 9 * HOUR));
        let change = mistaken.then(|| history.pick("apps", "fish", "fi", &[], when(5, 9 * HOUR + 1)));
        // The app the user meant, picked right after.
        history.pick(IDENTITIES, "app:firefox", "fi", &[], when(5, 9 * HOUR + 2));
        change
    };
    let mut withdrawn = used();
    let took_over = picks(&mut withdrawn, true).unwrap();
    assert_eq!(withdrawn.learned("fi", when(5, 9 * HOUR).at)[0].id, "fish");
    withdrawn.undo(took_over);
    let mut never = used();
    picks(&mut never, false);

    let (got, want) = (&withdrawn.learned["fi"], &never.learned["fi"]);
    assert_eq!(got.choice, want.choice);
    assert_eq!(got.contenders, want.contenders);
    assert_eq!(got.passed_over, want.passed_over);
    assert!(
        (got.strength - want.strength).abs() < 1e-3,
        "{} for {}",
        got.strength,
        want.strength
    );
}

#[test]
fn undo_leaves_alone_what_changed_since() {
    let mut history = used();
    let change = history.clear();
    history.touch("apps", "mail", when(4, 9 * HOUR));
    history.touch("apps", "fish", when(4, 9 * HOUR));
    let fish = *history.picks("apps", "fish").unwrap();
    history.undo(change);
    assert!(history.picks("apps", "mail").is_some(), "picked since");
    assert_eq!(history.picks("apps", "fish"), Some(&fish), "picked again since");
    assert!(history.picks(IDENTITIES, "app:firefox").is_some(), "put back");
    assert_eq!(history.learned("fi", when(4, 0).at)[0].id, "app:firefox");
}

#[test]
fn forgetting_what_one_extension_offered_keeps_the_rest() {
    let mut history = used();
    history.touch("files", "notes", when(0, 9 * HOUR));
    let before = kept(&history);
    let change =
        history.forget_where(|(extension, id)| extension == "apps" || (extension == IDENTITIES && id == "app:firefox"));
    let at = when(3, 0).at;
    assert!(history.learned("fi", at).is_empty());
    assert!(history.learned("sh", at).is_empty());
    let left: Vec<_> = history.picked().map(|(e, i, _)| (e, i)).collect();
    assert_eq!(left, [("files", "notes")]);
    history.undo(change);
    assert_eq!(kept(&history), before);
}

/// How much an app's picks count at `when`, of their decayed number.
fn counts(history: &History, id: &str, when: When) -> f64 {
    let picks = history.picks("apps", id).unwrap();
    let count = picks.count(when.at, history.settings().half_life_secs);
    history.frecency("apps", id, when) / count
}

use super::Power;
use super::items;

#[test]
fn hibernate_is_listed_only_when_logind_can_hibernate() {
    let with: Vec<String> = items(true).map(|item| item.id).collect();
    let without: Vec<String> = items(false).map(|item| item.id).collect();

    assert_eq!(with, ["power-off", "reboot", "suspend", "hibernate", "lock", "log-out"]);
    assert_eq!(without, ["power-off", "reboot", "suspend", "lock", "log-out"]);
}

#[test]
fn power_off_reboot_and_log_out_ask_first_and_the_rest_do_not() {
    let asking: Vec<Power> = Power::ALL.into_iter().filter(|power| power.asks_first()).collect();

    assert_eq!(asking, [Power::PowerOff, Power::Reboot, Power::LogOut]);
}

#[test]
fn the_items_that_ask_first_have_the_front_end_confirm_their_action() {
    let confirmed: Vec<Power> = Power::ALL
        .into_iter()
        .filter(|power| power.item().actions.iter().all(|action| action.confirm))
        .collect();

    assert_eq!(confirmed, [Power::PowerOff, Power::Reboot, Power::LogOut]);
}

#[test]
fn every_item_is_found_by_the_words_people_type() {
    let keywords = |power: Power| power.item().keywords;

    assert!(keywords(Power::PowerOff).contains(&"shutdown".to_owned()));
    assert!(keywords(Power::Reboot).contains(&"restart".to_owned()));
    assert!(keywords(Power::Suspend).contains(&"sleep".to_owned()));
    assert!(keywords(Power::LogOut).contains(&"logout".to_owned()));
}

#[test]
fn an_item_id_names_its_power_and_nothing_else_does() {
    for power in Power::ALL {
        assert_eq!(Power::from_id(power.id()), Some(power));
    }
    assert_eq!(Power::from_id("window-1"), None);
}

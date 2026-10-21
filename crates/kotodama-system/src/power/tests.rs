use kotodama_protocol::ViewContent;

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
fn a_confirmation_is_a_form_without_fields_that_names_what_it_asks_about() {
    let view = Power::PowerOff.confirmation();

    assert_eq!(
        view.content,
        ViewContent::Form {
            fields: Vec::new(),
            submit: "Power Off".into(),
        }
    );
    assert_eq!(Power::from_confirmation(&view.id), Some(Power::PowerOff));
}

#[test]
fn only_the_views_that_ask_name_a_power_item() {
    assert_eq!(Power::from_confirmation("confirm-suspend"), None);
    assert_eq!(Power::from_confirmation("power-off"), None);
    assert_eq!(Power::from_confirmation("confirm-log-out"), Some(Power::LogOut));
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

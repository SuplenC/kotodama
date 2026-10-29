use std::collections::BTreeMap;
use std::path::Path;

use kotodama_protocol::Argument;
use kotodama_protocol::FieldError;
use kotodama_protocol::FormField;
use kotodama_protocol::FormInput;
use kotodama_protocol::Table;
use kotodama_protocol::Value;
use kotodama_protocol::ViewContent;

use super::absolute;
use super::absolute_fields;
use super::absolute_paths;
use super::form;
use super::invalid;
use super::submitted;
use super::written_numbers;

const HOME: &str = "/home/kuba";

#[test]
fn a_tilde_expands_to_the_home_directory() {
    let home = Path::new(HOME);

    assert_eq!(absolute("~", home), "/home/kuba");
    assert_eq!(absolute("~/", home), "/home/kuba");
    assert_eq!(
        absolute("~/Downloads/work.conf", home),
        "/home/kuba/Downloads/work.conf"
    );
}

#[test]
fn a_relative_path_is_taken_from_the_home_directory() {
    let home = Path::new(HOME);

    assert_eq!(absolute("Downloads/work.conf", home), "/home/kuba/Downloads/work.conf");
    assert_eq!(absolute("./work.conf", home), "/home/kuba/./work.conf");
}

#[test]
fn only_a_tilde_alone_or_before_a_slash_is_the_home_directory() {
    assert_eq!(absolute("~bob/x", Path::new(HOME)), "/home/kuba/~bob/x");
}

#[test]
fn a_file_uri_is_its_decoded_path() {
    let home = Path::new(HOME);

    assert_eq!(
        absolute("file:///home/kuba/My%20VPN/work.conf", home),
        "/home/kuba/My VPN/work.conf"
    );
    assert_eq!(absolute("file://localhost/etc/x%2", home), "/etc/x%2");
}

#[test]
fn whitespace_around_a_path_goes() {
    let home = Path::new(HOME);

    assert_eq!(absolute("  ~/work.conf \n", home), "/home/kuba/work.conf");
    assert_eq!(absolute("   ", home), "");
}

#[test]
fn a_blank_form_value_is_left_out() {
    let values = Table::from([
        ("config".into(), Value::String("  ".into())),
        ("name".into(), Value::String("vpn".into())),
    ]);

    assert_eq!(
        submitted(values),
        BTreeMap::from([("name".to_owned(), "vpn".to_owned())])
    );
}

#[test]
fn an_absolute_or_empty_path_stays_as_it_is() {
    let home = Path::new(HOME);

    assert_eq!(absolute("/etc/wireguard/wg0.conf", home), "/etc/wireguard/wg0.conf");
    assert_eq!(absolute("", home), "");
}

#[test]
fn only_file_and_folder_arguments_become_absolute_paths() {
    let wanted = [
        Argument::new("config").file(["conf"]),
        Argument::new("into").folder(),
        Argument::new("name"),
        Argument::new("minutes").number(),
    ];
    let mut given = BTreeMap::from([
        ("config".to_owned(), "~/Downloads/work.conf".to_owned()),
        ("into".to_owned(), "vpn".to_owned()),
        ("name".to_owned(), "~work".to_owned()),
        ("minutes".to_owned(), "5".to_owned()),
    ]);

    absolute_paths(&wanted, &mut given, Path::new(HOME));

    assert_eq!(given["config"], "/home/kuba/Downloads/work.conf");
    assert_eq!(given["into"], "/home/kuba/vpn");
    assert_eq!(given["name"], "~work");
    assert_eq!(given["minutes"], "5");
}

#[test]
fn only_a_forms_file_fields_become_absolute_paths() {
    let fields = [
        FormField::new(
            "config",
            "Config",
            FormInput::File {
                accept: Vec::new(),
                folder: false,
                default: None,
                placeholder: None,
            },
        ),
        FormField::new(
            "name",
            "Name",
            FormInput::Text {
                placeholder: None,
                default: None,
                secret: false,
            },
        ),
    ];
    let mut values = Table::from([
        ("config".into(), Value::String("~/work.conf".into())),
        ("name".into(), Value::String("~work".into())),
    ]);

    absolute_fields(&fields, &mut values, Path::new(HOME));

    assert_eq!(values["config"], Value::String("/home/kuba/work.conf".into()));
    assert_eq!(values["name"], Value::String("~work".into()));
}

#[test]
fn a_file_argument_is_asked_for_in_a_file_field() {
    let wanted = [
        Argument::new("config")
            .file(["conf"])
            .placeholder("~/Downloads/work.conf"),
        Argument::new("into").folder(),
        Argument::new("name"),
    ];
    let given = BTreeMap::from([("into".to_owned(), "~/vpn".to_owned())]);

    let view = form("arguments-1".into(), "Import", &wanted, &given);

    let ViewContent::Form { fields, .. } = view.content else {
        panic!("{view:?}");
    };
    let inputs: Vec<FormInput> = fields.into_iter().map(|field| field.input).collect();
    assert_eq!(
        inputs,
        [
            FormInput::File {
                accept: vec!["conf".into()],
                folder: false,
                default: None,
                placeholder: Some("~/Downloads/work.conf".into()),
            },
            FormInput::File {
                accept: Vec::new(),
                folder: true,
                default: Some("~/vpn".into()),
                placeholder: None,
            },
            FormInput::Text {
                placeholder: None,
                default: None,
                secret: false,
            },
        ]
    );
}

#[test]
fn a_number_argument_is_asked_for_in_a_number_field_with_what_was_typed() {
    let wanted = [
        Argument::new("minutes").number(),
        Argument::new("seconds").number().optional(),
    ];
    let given = BTreeMap::from([("minutes".to_owned(), "25".to_owned())]);

    let view = form("arguments-1".into(), "Timer", &wanted, &given);

    let ViewContent::Form { fields, .. } = view.content else {
        panic!("{view:?}");
    };
    let number = |default| FormInput::Number {
        min: None,
        max: None,
        step: None,
        default,
    };
    assert_eq!(fields[0].input, number(Some(25.0)));
    assert_eq!(fields[1].input, number(None));
    assert_eq!((fields[0].optional, fields[1].optional), (false, true));
}

#[test]
fn a_number_argument_typed_as_anything_else_is_wrong() {
    let wanted = [Argument::new("minutes").number(), Argument::new("label")];
    let given = BTreeMap::from([
        ("minutes".to_owned(), "soon".to_owned()),
        ("label".to_owned(), "tea".to_owned()),
    ]);

    assert_eq!(
        invalid(&wanted, &given),
        [FieldError {
            field: "minutes".into(),
            message: "“soon” isn't a number".into(),
        }]
    );
    assert_eq!(
        invalid(&wanted, &BTreeMap::from([("minutes".to_owned(), " 2.5 ".to_owned())])),
        []
    );
}

#[test]
fn a_number_from_a_form_is_its_argument_as_text() {
    let values = Table::from([
        ("minutes".into(), Value::Integer(25)),
        ("ratio".into(), Value::Float(0.5)),
    ]);

    assert_eq!(
        submitted(values),
        BTreeMap::from([
            ("minutes".to_owned(), "25".to_owned()),
            ("ratio".to_owned(), "0.5".to_owned()),
        ])
    );
}

#[test]
fn a_number_argument_reaches_the_action_written_one_way() {
    let wanted = [
        Argument::new("a").number(),
        Argument::new("b").number(),
        Argument::new("c"),
    ];
    let mut given = BTreeMap::from([
        ("a".to_owned(), "1e3".to_owned()),
        ("b".to_owned(), " 2.50".to_owned()),
        ("c".to_owned(), "1e3".to_owned()),
    ]);

    written_numbers(&wanted, &mut given);

    assert_eq!(
        (given["a"].as_str(), given["b"].as_str(), given["c"].as_str()),
        ("1000", "2.5", "1e3")
    );
}

#[test]
fn a_blank_number_argument_is_wrong_as_a_missing_one_would_be_asked_for() {
    let wanted = [Argument::new("minutes").number()];

    assert_eq!(
        invalid(&wanted, &BTreeMap::from([("minutes".to_owned(), " ".to_owned())])).len(),
        1
    );
    assert_eq!(invalid(&wanted, &BTreeMap::new()), []);
}

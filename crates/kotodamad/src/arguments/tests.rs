use std::collections::BTreeMap;
use std::path::Path;

use kotodama_protocol::Argument;
use kotodama_protocol::FormField;
use kotodama_protocol::FormInput;
use kotodama_protocol::Table;
use kotodama_protocol::Value;
use kotodama_protocol::ViewContent;

use super::absolute;
use super::absolute_fields;
use super::absolute_paths;
use super::form;
use super::submitted;

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
    ];
    let mut given = BTreeMap::from([
        ("config".to_owned(), "~/Downloads/work.conf".to_owned()),
        ("into".to_owned(), "vpn".to_owned()),
        ("name".to_owned(), "~work".to_owned()),
    ]);

    absolute_paths(&wanted, &mut given, Path::new(HOME));

    assert_eq!(given["config"], "/home/kuba/Downloads/work.conf");
    assert_eq!(given["into"], "/home/kuba/vpn");
    assert_eq!(given["name"], "~work");
}

#[test]
fn only_a_forms_file_fields_become_absolute_paths() {
    let fields = [
        FormField {
            id: "config".into(),
            label: "Config".into(),
            input: FormInput::File {
                accept: Vec::new(),
                folder: false,
                default: None,
                placeholder: None,
            },
        },
        FormField {
            id: "name".into(),
            label: "Name".into(),
            input: FormInput::Text {
                placeholder: None,
                default: None,
                secret: false,
            },
        },
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

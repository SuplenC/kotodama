use kotodama_protocol::FieldError;
use kotodama_protocol::FormField;
use kotodama_protocol::FormInput;
use kotodama_protocol::SelectOption;
use kotodama_protocol::Table;
use kotodama_protocol::Value;
use kotodama_protocol::View;
use kotodama_protocol::ViewContent;

use super::check;
use super::drop_bad_patterns;
use super::number;

fn text() -> FormInput {
    FormInput::Text {
        placeholder: None,
        default: None,
        secret: false,
    }
}

fn number_input(min: Option<f64>, max: Option<f64>, step: Option<f64>) -> FormInput {
    FormInput::Number {
        min,
        max,
        step,
        default: None,
    }
}

fn date(time: bool) -> FormInput {
    FormInput::Date { time, default: None }
}

fn options(values: &[&str]) -> Vec<SelectOption> {
    values
        .iter()
        .map(|value| SelectOption {
            value: (*value).into(),
            label: value.to_uppercase(),
        })
        .collect()
}

fn multi_select(values: &[&str]) -> FormInput {
    FormInput::MultiSelect {
        options: options(values),
        default: Vec::new(),
    }
}

fn string(text: &str) -> Value {
    Value::String(text.into())
}

fn strings(texts: &[&str]) -> Value {
    Value::Array(texts.iter().map(|text| string(text)).collect())
}

/// The errors `check` finds in `value` for a form of the one field `field`,
/// and the value it leaves, as the Extension would get it.
fn checked(field: &FormField, value: Option<Value>) -> (Vec<String>, Option<Value>) {
    let mut values = Table::new();
    if let Some(value) = value {
        values.insert(field.id.clone(), value);
    }

    let errors = check(std::slice::from_ref(field), &mut values);

    let messages = errors
        .into_iter()
        .map(|error| {
            assert_eq!(error.field, field.id);
            error.message
        })
        .collect();
    (messages, values.remove(&field.id))
}

fn messages(field: &FormField, value: Option<Value>) -> Vec<String> {
    checked(field, value).0
}

#[test]
fn a_required_field_left_blank_or_out_is_required() {
    let name = || FormField::new("name", "Name", text());

    assert_eq!(messages(&name(), None), ["Required"]);
    assert_eq!(messages(&name(), Some(string("  "))), ["Required"]);
    assert_eq!(messages(&name(), Some(string("kuba"))), Vec::<String>::new());
}

#[test]
fn an_optional_field_may_be_left_blank() {
    let name = || FormField::new("name", "Name", text()).optional();

    assert_eq!(messages(&name(), None), Vec::<String>::new());
    assert_eq!(messages(&name(), Some(string(""))), Vec::<String>::new());
}

#[test]
fn a_checkbox_is_never_missing_and_a_description_has_no_value() {
    let tick = FormField::new("tick", "Tick", FormInput::Checkbox { default: false });
    let about = FormField::new("about", "", FormInput::Description { text: "Read me".into() });

    assert_eq!(messages(&tick, None), Vec::<String>::new());
    assert_eq!(checked(&about, Some(string("sent anyway"))), (Vec::new(), None));
}

#[test]
fn a_value_of_the_wrong_type_is_wrong() {
    let tick = FormField::new("tick", "Tick", FormInput::Checkbox { default: false });

    assert_eq!(
        messages(&FormField::new("name", "Name", text()), Some(Value::Bool(true))),
        ["Expects text"]
    );
    assert_eq!(messages(&tick, Some(string("yes"))), ["Expects true or false"]);
}

#[test]
fn a_number_is_a_number_the_field_takes() {
    let percent = || FormField::new("percent", "Percent", number_input(Some(0.0), Some(100.0), None));

    assert_eq!(messages(&percent(), Some(string("loud"))), ["Not a number"]);
    assert_eq!(messages(&percent(), Some(string("NaN"))), ["Not a number"]);
    assert_eq!(messages(&percent(), Some(Value::Integer(-1))), ["At least 0"]);
    assert_eq!(messages(&percent(), Some(Value::Float(100.5))), ["At most 100"]);
    assert_eq!(messages(&percent(), Some(Value::Integer(100))), Vec::<String>::new());
}

#[test]
fn a_number_goes_in_steps_from_its_least() {
    let halves = || FormField::new("zoom", "Zoom", number_input(Some(1.0), None, Some(0.5)));
    let tenths = || FormField::new("gain", "Gain", number_input(None, None, Some(0.1)));

    assert_eq!(messages(&halves(), Some(Value::Float(2.5))), Vec::<String>::new());
    assert_eq!(
        messages(&halves(), Some(Value::Float(2.25))),
        ["In steps of 0.5 from 1"]
    );
    assert_eq!(messages(&tenths(), Some(Value::Float(0.3))), Vec::<String>::new());
    assert_eq!(messages(&tenths(), Some(Value::Float(0.35))), ["In steps of 0.1"]);
}

#[test]
fn a_number_reaches_the_extension_as_an_integer_when_it_is_whole() {
    let any = || FormField::new("n", "N", number_input(None, None, None));

    assert_eq!(
        checked(&any(), Some(string(" 25 "))),
        (Vec::new(), Some(Value::Integer(25)))
    );
    assert_eq!(
        checked(&any(), Some(Value::Float(3.0))),
        (Vec::new(), Some(Value::Integer(3)))
    );
    assert_eq!(
        checked(&any(), Some(string("2.5"))),
        (Vec::new(), Some(Value::Float(2.5)))
    );
}

#[test]
fn an_optional_number_left_blank_reaches_the_extension_left_out() {
    let field = FormField::new("n", "N", number_input(None, None, None)).optional();

    assert_eq!(checked(&field, Some(string(""))), (Vec::new(), None));
}

#[test]
fn a_date_is_an_iso_8601_date_and_with_time_a_local_date_and_time() {
    let day = || FormField::new("on", "On", date(false));
    let moment = || FormField::new("at", "At", date(true));

    assert_eq!(messages(&day(), Some(string("2026-10-30"))), Vec::<String>::new());
    assert_eq!(messages(&day(), Some(string("2028-02-29"))), Vec::<String>::new());
    assert_eq!(
        messages(&moment(), Some(string("2026-10-30T09:30"))),
        Vec::<String>::new()
    );
    assert_eq!(
        messages(&moment(), Some(string("2026-10-30T23:59:59"))),
        Vec::<String>::new()
    );
    for wrong in [
        "2026-02-29",
        "2026-13-01",
        "2026-10-30T09:30",
        "30.10.2026",
        "2026-1-30",
    ] {
        assert_eq!(
            messages(&day(), Some(string(wrong))),
            ["Not a date: write it as YYYY-MM-DD"],
            "{wrong}"
        );
    }
    for wrong in [
        "2026-10-30",
        "2026-10-30T24:00",
        "2026-10-30T09:60",
        "2026-10-30T09:30Z",
    ] {
        assert_eq!(
            messages(&moment(), Some(string(wrong))),
            ["Not a date and time: write it as YYYY-MM-DDTHH:MM"],
            "{wrong}"
        );
    }
}

#[test]
fn a_multi_select_chooses_among_its_options() {
    let tags = || FormField::new("tags", "Tags", multi_select(&["a", "b"]));

    assert_eq!(messages(&tags(), Some(strings(&["b", "a"]))), Vec::<String>::new());
    assert_eq!(
        messages(&tags(), Some(strings(&["a", "c"]))),
        ["“c” isn't one of the options"]
    );
    assert_eq!(messages(&tags(), Some(strings(&[]))), ["Required"]);
    assert_eq!(messages(&tags(), Some(string("a"))), ["Expects a list of options"]);
}

#[test]
fn a_select_chooses_one_of_its_options() {
    let channel = || {
        FormField::new(
            "channel",
            "Channel",
            FormInput::Select {
                options: options(&["stable", "beta"]),
                default: None,
            },
        )
    };

    assert_eq!(messages(&channel(), Some(string("beta"))), Vec::<String>::new());
    assert_eq!(
        messages(&channel(), Some(string("nightly"))),
        ["“nightly” isn't one of the options"]
    );
}

#[test]
fn a_pattern_matches_the_whole_text() {
    let code = || FormField::new("code", "Code", text()).pattern("[0-9]{6}");

    assert_eq!(messages(&code(), Some(string("123456"))), Vec::<String>::new());
    assert_eq!(
        messages(&code(), Some(string("1234567"))),
        ["Not in the form this field takes"]
    );
    assert_eq!(
        messages(&code(), Some(string("x123456"))),
        ["Not in the form this field takes"]
    );
}

#[test]
fn an_optional_field_left_blank_skips_its_pattern() {
    let code = FormField::new("code", "Code", text()).pattern("[0-9]{6}").optional();

    assert_eq!(messages(&code, Some(string(""))), Vec::<String>::new());
}

#[test]
fn every_wrong_field_is_named_in_the_forms_order() {
    let fields = [
        FormField::new("name", "Name", text()),
        FormField::new("tick", "Tick", FormInput::Checkbox { default: false }),
        FormField::new("n", "N", number_input(None, Some(3.0), None)),
    ];
    let mut values = Table::from([("n".into(), Value::Integer(4))]);

    let errors = check(&fields, &mut values);

    assert_eq!(
        errors,
        [
            FieldError {
                field: "name".into(),
                message: "Required".into(),
            },
            FieldError {
                field: "n".into(),
                message: "At most 3".into(),
            },
        ]
    );
}

#[test]
fn a_typed_number_is_one_in_any_form_rust_reads() {
    assert_eq!(number(&string("1e3")), Some(1000.0));
    assert_eq!(number(&string("-2.5")), Some(-2.5));
    assert_eq!(number(&string("inf")), None);
    assert_eq!(number(&string("")), None);
}

#[test]
fn a_pattern_that_isnt_a_regular_expression_is_dropped() {
    let mut view = View {
        id: "new".into(),
        title: "New".into(),
        content: ViewContent::Form {
            fields: vec![
                FormField::new("good", "Good", text()).pattern("[a-z]+"),
                FormField::new("bad", "Bad", text()).pattern("[a-z"),
            ],
            submit: "Go".into(),
        },
    };

    let dropped = drop_bad_patterns(&mut view);

    assert_eq!(dropped.len(), 1);
    assert!(
        dropped[0].starts_with("form field `bad`'s pattern `[a-z` isn't a regular expression"),
        "{dropped:?}"
    );
    let ViewContent::Form { fields, .. } = &view.content else {
        unreachable!()
    };
    assert_eq!(fields[0].pattern.as_deref(), Some("[a-z]+"));
    assert_eq!(fields[1].pattern, None);
}

#[test]
fn a_large_number_on_a_fractional_step_is_on_it() {
    let tenths = FormField::new("n", "N", number_input(None, None, Some(0.1)));

    assert_eq!(
        messages(&tenths, Some(Value::Float(10_000_000.1))),
        Vec::<String>::new()
    );
}

#[test]
fn a_date_loses_the_whitespace_around_it() {
    let day = FormField::new("on", "On", date(false));

    assert_eq!(
        checked(&day, Some(string(" 2026-10-30 "))),
        (Vec::new(), Some(string("2026-10-30")))
    );
}

#[test]
fn a_pattern_whose_comment_swallows_the_anchors_is_dropped_rather_than_let_anything_through() {
    let mut view = View {
        id: "new".into(),
        title: "New".into(),
        content: ViewContent::Form {
            fields: vec![FormField::new("code", "Code", text()).pattern("(?x)[0-9]{6} # six digits")],
            submit: "Go".into(),
        },
    };

    assert_eq!(drop_bad_patterns(&mut view).len(), 1);
    let code = FormField::new("code", "Code", text()).pattern("(?x)[0-9]{6} # six digits");
    assert_eq!(
        messages(&code, Some(string("anything"))),
        ["Not in the form this field takes"]
    );
}

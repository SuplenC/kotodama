use super::*;

fn args(line: &str) -> Vec<String> {
    line.split(' ').map(String::from).collect()
}

#[test]
fn open_takes_a_query_and_a_view_in_any_order() {
    assert_eq!(
        parse(&args("open --view clipboard --query fir")),
        Ok(FromFrontEnd::Open {
            query: "fir".into(),
            view: Some("clipboard".into()),
        })
    );
    assert_eq!(
        parse(&args("open")),
        Ok(FromFrontEnd::Open {
            query: String::new(),
            view: None,
        })
    );
}

#[test]
fn open_behind_a_prefix_types_it_before_the_query() {
    assert_eq!(
        parse(&args("open --prefix emoji")),
        Ok(FromFrontEnd::Open {
            query: "emoji ".into(),
            view: None,
        })
    );
    assert_eq!(
        parse(&args("open --query fire --prefix emoji")),
        Ok(FromFrontEnd::Open {
            query: "emoji fire".into(),
            view: None,
        })
    );
}

#[test]
fn a_flag_without_a_value_or_an_unknown_one_is_an_error() {
    assert!(parse(&args("open --query")).is_err());
    assert!(parse(&args("open --size 3")).is_err());
    assert!(parse(&args("toggle now")).is_err());
    assert!(parse(&[]).is_err());
}

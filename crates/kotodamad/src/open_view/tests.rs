
use super::*;

#[test]
fn a_view_name_is_an_extension_and_an_item_id_that_may_hold_slashes() {
    assert_eq!(
        ViewName::parse("folders/src/kotodama"),
        Some(ViewName {
            extension: "folders".into(),
            item: "src/kotodama".into(),
        })
    );
    assert_eq!(
        ViewName::parse("clipboard/history").map(|name| name.to_string()),
        Some("clipboard/history".into())
    );
    for bad in ["history", "/history", "clipboard/", ""] {
        assert_eq!(ViewName::parse(bad), None, "{bad}");
    }
}

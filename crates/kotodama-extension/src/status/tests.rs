use super::*;

#[test]
fn each_change_is_sent_once() {
    let mut status = SentStatus::default();

    let first = status.update(false, Some("Hyprland isn't running"));
    let again = status.update(false, Some("Hyprland isn't running"));
    let up = status.update(true, None);
    let noted = status.update(true, Some("Paused"));

    assert_eq!(
        first,
        Some(FromExtension::Status {
            usable: false,
            note: Some("Hyprland isn't running".into()),
        })
    );
    assert_eq!(again, None);
    assert_eq!(
        up,
        Some(FromExtension::Status {
            usable: true,
            note: None,
        })
    );
    assert!(noted.is_some());
}

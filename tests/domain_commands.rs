use mzed::domain::action::AppCommand;
use mzed::domain::navigation::{NavigationEffect, Pane, PaneLayout};

#[test]
fn keyboard_messages_decode_to_typed_commands() {
    let command = AppCommand::from_value(&serde_json::json!({
        "kind": "focus_pane",
        "index": 1
    }))
    .unwrap();

    assert_eq!(command, AppCommand::FocusPane { index: 1 });
}

#[test]
fn unknown_keyboard_messages_are_rejected() {
    let error = AppCommand::from_value(&serde_json::json!({
        "kind": "execute_arbitrary_code"
    }))
    .unwrap_err();

    assert!(error.to_string().contains("unknown variant"));
}

#[test]
fn focusing_the_right_pane_opens_a_missing_split() {
    let mut layout = PaneLayout::default();

    let effect = layout.focus(Pane::Right);

    assert_eq!(effect, NavigationEffect::OpenRight);
    assert!(layout.is_split());
    assert_eq!(layout.focused(), Pane::Right);
}

#[test]
fn collapsing_a_split_returns_focus_to_the_left_pane() {
    let mut layout = PaneLayout::default();
    layout.focus(Pane::Right);

    let effect = layout.toggle_split();

    assert_eq!(effect, NavigationEffect::CloseRight);
    assert!(!layout.is_split());
    assert_eq!(layout.focused(), Pane::Left);
}

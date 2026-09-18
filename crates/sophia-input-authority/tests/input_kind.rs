use sophia_input_authority::{Input, InputKind};

#[test]
fn checked_inputs_report_their_own_class_across_both_domains() {
    for key in Input::MIN_KEYCODE..=u8::MAX {
        assert_eq!(Input::key(key).unwrap().kind(), InputKind::Key);
    }
    for button in 1..=9 {
        assert_eq!(Input::button(button, 9).unwrap().kind(), InputKind::Button);
    }
    assert!(Input::key(7).is_err());
    assert!(Input::button(0, 9).is_err());
    assert!(Input::button(10, 9).is_err());
}

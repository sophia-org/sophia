#![cfg(test)]
use super::*;

#[test]
fn native_withdrawal_stales_retained_readers_without_a_session_poll() {
    let owner = CaptureValidityOwner::default();
    let retained = owner.0.clone();
    assert_eq!(retained.with_current(|| 7), Some(7));
    drop(owner);
    assert_eq!(
        retained.with_current(|| panic!("withdrawn read ran")),
        None::<()>
    );
    let replacement = CaptureValidityOwner::default();
    assert_eq!(replacement.0.with_current(|| 8), Some(8));
    assert_eq!(retained.with_current(|| 9), None);
}

#[test]
fn native_withdrawal_serializes_with_an_existing_delivery() {
    let owner = CaptureValidityOwner::default();
    let retained = owner.0.clone();
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let reader = std::thread::spawn(move || {
        retained.with_current(|| {
            entered_tx.send(()).unwrap();
            release_rx.recv().unwrap();
        })
    });
    entered_rx.recv().unwrap();
    assert!(owner.0.0.try_lock().is_err());
    let retained = owner.0.clone();
    let withdrawal = std::thread::spawn(move || drop(owner));
    release_tx.send(()).unwrap();
    assert_eq!(reader.join().unwrap(), Some(()));
    withdrawal.join().unwrap();
    assert_eq!(retained.with_current(|| 9), None);
}

use sophia_portal::execution_fence::PortalExecutionFence;
use std::{sync::mpsc, time::Duration};

#[test]
fn unlock_cannot_revive_an_old_execution_and_close_is_permanent() {
    let fence = PortalExecutionFence::default();
    let old = fence.revision();
    assert_eq!(fence.execute(old, u64::MAX, || 7), Some(7));
    fence.block();
    assert!(!fence.live());
    fence.unblock();
    assert!(fence.live());
    assert_eq!(
        fence.execute(old, u64::MAX, || panic!("old execution revived")),
        None::<()>
    );
    assert_eq!(
        fence.execute(fence.revision(), 0, || panic!("expired effect")),
        None::<()>
    );
    assert_eq!(fence.execute(fence.revision(), u64::MAX, || 8), Some(8));
    fence.close();
    fence.unblock();
    assert!(!fence.live());
    assert_eq!(
        fence.execute(fence.revision(), u64::MAX, || panic!("closed effect")),
        None::<()>
    );
}

#[test]
fn lock_transition_waits_for_the_recipient_effect_not_transport_progress() {
    let fence = PortalExecutionFence::default();
    let revision = fence.revision();
    let effect = fence.clone();
    let (entered, inside) = mpsc::sync_channel(1);
    let (release, released) = mpsc::sync_channel(1);
    let worker = std::thread::spawn(move || {
        effect.execute(revision, u64::MAX, || {
            entered.send(()).unwrap();
            released.recv_timeout(Duration::from_secs(2)).unwrap();
        })
    });
    inside.recv_timeout(Duration::from_secs(1)).unwrap();
    let (done, finished) = mpsc::sync_channel(1);
    let locking = fence.clone();
    let transition = std::thread::spawn(move || {
        locking.block();
        done.send(()).unwrap();
    });
    let early = finished.recv_timeout(Duration::from_millis(30));
    release.send(()).unwrap();
    assert!(
        early.is_err(),
        "lock cannot pass an in-progress recipient effect"
    );
    finished.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(worker.join().unwrap(), Some(()));
    transition.join().unwrap();
    assert!(!fence.live());
}

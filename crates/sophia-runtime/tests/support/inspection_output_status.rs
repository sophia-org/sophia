#![cfg(test)]
use super::*;
use sophia_9p::Errno;

fn output(availability: OutputAvailability, owner: u64) -> OutputStatus {
    OutputStatus {
        availability,
        topology_generation: 3,
        owner_generation: owner,
        owner_retirements: 0,
        recovery_stage: "stable",
        retry_at_session_msec: None,
        failure_stage: "none",
        failure_code: "none",
        failure_errno: 0,
    }
}

fn read_text(owner: &mut InspectionExport, handle: &mut super::super::export::Handle) -> String {
    let ReadOutcome::Ready(bytes) = owner.read(&Node::OutputStatus, handle, 0, 4096).unwrap()
    else {
        panic!("status must be a bounded immediate read")
    };
    String::from_utf8(bytes).unwrap()
}

#[test]
fn output_status_reports_loss_even_when_policy_geometry_is_retained() {
    let (shared, publisher, _wake) = setup();
    let (server, _client) = UnixStream::pair().unwrap();
    let mut owner = export(shared.clone(), &server);
    assert!(matches!(
        owner.open(&Node::OutputStatus, OpenFlags(0)),
        Err(Errno::EAGAIN)
    ));
    assert!(publisher.publish_output_status(output(OutputAvailability::Available, 7)));
    let mut before = owner.open(&Node::OutputStatus, OpenFlags(0)).unwrap();
    assert!(read_text(&mut owner, &mut before).contains("availability=available"));
    let mut waiting = output(OutputAvailability::Waiting, 0);
    waiting.recovery_stage = "quarantined";
    waiting.retry_at_session_msec = Some(5000);
    assert!(publisher.publish_output_status(waiting));
    let mut after = owner.open(&Node::OutputStatus, OpenFlags(0)).unwrap();
    let text = read_text(&mut owner, &mut after);
    assert!(text.contains("availability=waiting"));
    assert!(text.contains("topology_generation=3 owner_generation=0"));
    assert!(text.contains("retry_at_session_msec=5000"));
    // An open status file is an observation, not current capture authority.
    assert!(read_text(&mut owner, &mut before).contains("availability=available"));
    assert!(text.contains("capture_authority=none"));
    assert_eq!(
        shared
            .view()
            .unwrap()
            .snapshot
            .as_ref()
            .unwrap()
            .record
            .snapshot,
        value()
    );
    shared.fence(10, None).unwrap();
    assert_eq!(
        owner.read(&Node::OutputStatus, &mut before, 0, 4096),
        Err(Errno::ESTALE)
    );
    let mut fresh = export(shared, &server);
    assert!(matches!(
        fresh.open(&Node::OutputStatus, OpenFlags(0)),
        Err(Errno::EAGAIN)
    ));
}

#[test]
fn output_status_contention_withdraws_new_observations_until_republished() {
    let (shared, publisher, _wake) = setup();
    let (server, _client) = UnixStream::pair().unwrap();
    let mut owner = export(shared.clone(), &server);
    let ready = output(OutputAvailability::Available, 7);
    assert!(publisher.publish_output_status(ready));
    let held = shared.output_status.lock().unwrap();
    assert!(!publisher.publish_output_status(output(OutputAvailability::Waiting, 0)));
    drop(held);
    assert!(matches!(
        owner.open(&Node::OutputStatus, OpenFlags(0)),
        Err(Errno::EAGAIN)
    ));
    assert!(publisher.publish_output_status(ready));
    assert!(owner.open(&Node::OutputStatus, OpenFlags(0)).is_ok());
}

#[test]
fn output_status_never_formats_unbounded_error_text_or_available_without_owner() {
    let (shared, publisher, _wake) = setup();
    let (server, _client) = UnixStream::pair().unwrap();
    let mut owner = export(shared, &server);
    for bad in [
        output(OutputAvailability::Available, 0),
        OutputStatus {
            failure_code: "error\nsecret=path",
            ..output(OutputAvailability::Waiting, 0)
        },
        OutputStatus {
            owner_retirements: 1,
            ..output(OutputAvailability::Waiting, 0)
        },
    ] {
        publisher.publish_output_status(bad);
        assert!(matches!(
            owner.open(&Node::OutputStatus, OpenFlags(0)),
            Err(Errno::EIO)
        ));
    }
    assert!(matches!(
        owner.open(&Node::OutputStatus, OpenFlags(1)),
        Err(Errno::EACCES)
    ));
}

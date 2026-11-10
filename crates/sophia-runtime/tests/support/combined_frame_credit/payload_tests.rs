use super::tests::WAIT;
use super::*;
use std::io::Read;
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;

#[test]
fn real_worker_moves_the_plan_and_only_the_owner_drops_the_returned_descriptor() {
    let mut worker = IsbValidationWorker::new(2, 8).unwrap();
    let token = tests::reserve(&mut worker);
    assert!(!token.was_forwarded());
    let (mut reader, writer) = UnixStream::pair().unwrap();
    reader.set_nonblocking(true).unwrap();
    let raw = writer.as_raw_fd();
    let (sender, receiver) = mpsc::sync_channel(1);
    let owner_plan = worker
        .submit_combined(
            &token,
            tests::source(&[0; 8]),
            ProbePlan {
                id: 917,
                descriptor: writer.into(),
            },
            sender,
        )
        .ok()
        .expect("plan submitted with real validation");
    let returned = receiver.recv_timeout(WAIT).unwrap();
    // Receiving can precede the Release store; this ordinary FIFO job joins it.
    let barrier = worker
        .submit(tests::source(&[0; 8]), 8, Instant::now() + WAIT)
        .unwrap();
    assert_eq!(tests::completion(&mut worker).job, barrier);
    assert!(
        token.was_forwarded(),
        "successful move publishes its witness"
    );
    assert!(
        owner_plan.try_recv().is_err(),
        "forwarded plan cannot return twice"
    );
    assert!(worker.cancel(token.job));
    assert_eq!(worker.pending(), 1);
    let (pixels, plan) = worker
        .settle_combined(token, returned)
        .ok()
        .expect("owner consumes the exact returned plan");
    assert_eq!(pixels.bytes(), &[0; 8]);
    assert_eq!(plan.id, 917);
    assert_eq!(plan.descriptor.as_raw_fd(), raw);
    let mut byte = [0];
    assert_eq!(
        reader.read(&mut byte).unwrap_err().kind(),
        io::ErrorKind::WouldBlock
    );
    drop(plan);
    assert_eq!(reader.read(&mut byte).unwrap(), 0);
    assert_eq!(worker.pending(), 0);
}

#[test]
fn generic_transport_requires_no_payload_inspection_traits() {
    // Distinct from the concrete Job fixture payload, without Clone or Debug.
    struct MoveOnly(OwnedFd);
    let mut worker = IsbValidationWorker::new(1, 8).unwrap();
    let job = worker
        .submit(tests::source(&[0; 8]), 8, Instant::now() + WAIT)
        .unwrap();
    let pixels = tests::completion(&mut worker).result.unwrap();
    let identity = Arc::new(Identity {
        forwarded: AtomicBool::new(false),
    });
    let payload = MoveOnly(tests::source(&[9; 8]));
    let raw = payload.0.as_raw_fd();
    let (sender, receiver) = mpsc::sync_channel(1);
    let (owner_plan, returned_plan) = mpsc::sync_channel(1);
    assert!(
        forward(
            Some(Route {
                identity: Arc::clone(&identity),
                plan: payload,
                sender,
                owner_plan,
            }),
            job,
            Ok(pixels),
        )
        .is_none()
    );
    let returned = receiver.try_recv().ok().expect("generic envelope");
    assert_eq!(returned.plan.0.as_raw_fd(), raw);
    assert_eq!(returned.pixels.bytes(), &[0; 8]);
    assert!(identity.forwarded.load(Ordering::Acquire));
    assert!(returned_plan.try_recv().is_err());
}

#[test]
fn abandoning_the_plan_return_does_not_block_the_ordinary_failure_route() {
    let mut worker = IsbValidationWorker::new(1, 8).unwrap();
    let token = tests::reserve(&mut worker);
    assert!(worker.cancel(token.job));
    let (mut reader, writer) = UnixStream::pair().unwrap();
    reader.set_nonblocking(true).unwrap();
    let (sender, receiver) = mpsc::sync_channel(1);
    let owner_plan = worker
        .submit_combined(
            &token,
            tests::source(&[0; 8]),
            ProbePlan {
                id: 55,
                descriptor: writer.into(),
            },
            sender,
        )
        .ok()
        .expect("cancelled job accepted for completion");
    drop(owner_plan);
    let result = tests::completion(&mut worker);
    assert_eq!(result.result.err(), Some(IsbValidationFailure::Cancelled));
    assert!(!token.was_forwarded());
    assert!(receiver.try_recv().is_err());
    assert_eq!(worker.pending(), 0);
    assert_eq!(reader.read(&mut [0]).unwrap(), 0);
}

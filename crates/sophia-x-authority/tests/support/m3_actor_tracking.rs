use super::*;

#[test]
fn dropping_a_closed_service_retires_its_exact_actor_origin() {
    let mut service = LifecycleService::launch("actor-drop", 11800, None, false);
    // Keep the allocation alive: this checks the abandoned origin directly,
    // without asking a later service to happen to reuse its heap address.
    let registry = service.registry.clone();
    let origin = Arc::as_ptr(&registry.clients) as usize;
    let service_thread = service.handle.as_ref().unwrap().thread().id();
    service.start();
    service.command(XServerFrontendServiceCommand::StopAndDisconnect);
    assert!(service.closed().succeeded);
    let recorded = ACTORS
        .lock()
        .unwrap()
        .iter()
        .any(|actor| actor.origin == origin && actor.thread == service_thread && !actor.joined);
    assert!(recorded, "the real service actor is awaiting its join");

    drop(service);

    let remaining = ACTORS
        .lock()
        .unwrap()
        .iter()
        .filter(|actor| actor.origin == origin)
        .map(|actor| (actor.kind, actor.thread, actor.joined))
        .collect::<Vec<_>>();
    let tracked = TRACKED_ORIGINS.lock().unwrap().contains(&origin);
    assert!(
        remaining.is_empty(),
        "dropped service left actors for its pinned origin: {remaining:?}"
    );
    assert!(!tracked, "dropped service left its pinned origin tracked");
}

#[test]
fn an_unjoined_actor_still_fails_finish_and_drop_without_poisoning_bookkeeping() {
    for finish in [true, false] {
        let mut service = LifecycleService::launch("actor-missing-join", 11801, None, false);
        let registry = service.registry.clone();
        let origin = Arc::as_ptr(&registry.clients) as usize;
        service.start();
        service.command(XServerFrontendServiceCommand::StopAndDisconnect);
        assert!(service.closed().succeeded);

        // This real handle is deliberately withheld from both cleanup paths.
        // Its eventual join below cannot satisfy their earlier assertion.
        let withheld = std::thread::spawn(|| {});
        actor_started(
            &registry,
            withheld.thread().id(),
            "withheld-negative-control",
        );
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if finish {
                service.finish(&[]);
            } else {
                drop(service);
            }
        }));
        withheld
            .join()
            .expect("collect the negative control's real handle");
        let failure = result.expect_err("an unjoined actor must fail collection");
        let message = failure
            .downcast_ref::<String>()
            .expect("join assertion text");
        assert!(
            message.contains("actual started actor remains uncollected: withheld-negative-control"),
            "the missing join caused the failure: {message}"
        );
        assert!(
            !ACTORS.is_poisoned(),
            "collection asserted outside the actor lock"
        );
        assert!(!TRACKED_ORIGINS.is_poisoned());
        let remaining = ACTORS
            .lock()
            .unwrap()
            .iter()
            .any(|actor| actor.origin == origin);
        let tracked = TRACKED_ORIGINS.lock().unwrap().contains(&origin);
        assert!(!remaining, "failed collection retired its actor records");
        assert!(!tracked, "failed collection retired its tracked origin");
    }
}

//! Real supervised app lifecycle. Scene/input are absent; inspection only
//! supplies the bounded role owner needed before reporting Started.
#![cfg(test)]
use super::*;
use sophia_runtime::inspection::InspectionService;

#[path = "namespace_application_diagnostics.rs"]
mod diagnostics;

fn application(
    f: &mut Fixture,
    seconds: &str,
    dispatch: bool,
) -> (ApplicationProcess, InspectionService) {
    let mut request = f.request();
    request.launch.args = vec![seconds.into()];
    application_request(f, request, dispatch)
}

fn application_request(
    f: &mut Fixture,
    mut request: ClientLaunchRequest,
    dispatch: bool,
) -> (ApplicationProcess, InspectionService) {
    let mut service = InspectionService::private().unwrap();
    service.fence(1, None).unwrap();
    request
        .approved
        .get_mut("inspection")
        .unwrap()
        .service
        .generation = service.generation().unwrap();
    let reference = request.approved["inspection"].service.clone();
    let services = NamespaceApplicationServices {
        inspection: dispatch.then(|| {
            (
                reference.clone(),
                service
                    .authenticated_acceptor(reference.generation)
                    .unwrap(),
            )
        }),
        portal: None,
    };
    let pending = f
        .service
        .start(
            request,
            &mut f.auth.coordinator,
            f.wake.notifier(),
            Instant::now(),
        )
        .unwrap();
    (ApplicationProcess::namespace(pending, services), service)
}
fn poll(f: &mut Fixture, app: &mut ApplicationProcess) -> Option<ApplicationProcessEvent> {
    f.auth.tick();
    app.poll(&mut f.auth.coordinator, f.wake.notifier(), Instant::now())
}

#[test]
fn namespace_application_reports_started_and_exit_once_and_releases_custody() {
    let mut f = Fixture::new();
    let (mut app, _service) = application(&mut f, "1", true);
    assert_eq!(
        app.process_id(),
        None,
        "preparation has no process identity"
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut started = false;
    loop {
        assert!(Instant::now() < deadline, "application lifecycle deadline");
        match poll(&mut f, &mut app) {
            Some(ApplicationProcessEvent::Started { process, peer }) => {
                assert!(!started, "started is delivered exactly once");
                assert_ne!(peer, 0);
                assert_eq!(app.process_id(), Some(process));
                assert_eq!(f.service.occupied(), 1);
                started = true;
            }
            Some(ApplicationProcessEvent::Exited(status)) => {
                assert!(started, "dispatch is installed before Started");
                assert!(status.success());
                break;
            }
            Some(ApplicationProcessEvent::Failed(reason)) => {
                panic!("application startup: {reason}")
            }
            None => {}
        }
    }
    assert_eq!(app.process_id(), None);
    assert!(poll(&mut f, &mut app).is_none(), "exit must not repeat");
    f.reclaimed();
}

#[test]
fn missing_dispatch_is_a_local_terminal_failure_and_reaps_the_child() {
    let mut f = Fixture::new();
    let (mut app, _service) = application(&mut f, "30", false);
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        assert!(Instant::now() < deadline, "dispatch refusal deadline");
        match poll(&mut f, &mut app) {
            Some(ApplicationProcessEvent::Failed(reason)) => {
                assert_eq!(reason, "application has an undispatched namespace endpoint");
                break;
            }
            None => {}
            _ => panic!("an unserved application must never be reported started"),
        }
    }
    assert!(poll(&mut f, &mut app).is_none(), "failure must not repeat");
    f.reclaimed();
}

#[test]
fn cancellation_covers_preparation_and_running_without_owner_thread_reaping() {
    for running in [false, true] {
        let mut f = Fixture::new();
        let (mut app, _service) = application(&mut f, "30", true);
        if running {
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                assert!(Instant::now() < deadline, "application startup deadline");
                match poll(&mut f, &mut app) {
                    Some(ApplicationProcessEvent::Started { .. }) => break,
                    None => {}
                    _ => panic!("application must start before running cancellation"),
                }
            }
        }
        let before = Instant::now();
        assert!(app.cancel_namespace());
        assert!(
            before.elapsed() < Duration::from_secs(1),
            "cancellation cannot wait for child reaping"
        );
        assert!(poll(&mut f, &mut app).is_none());
        f.reclaimed();
    }
}

#[test]
fn ordinary_host_process_keeps_existing_child_custody_after_exit_observation() {
    let mut f = Fixture::new();
    let child = std::process::Command::new("/usr/bin/true").spawn().unwrap();
    let pid = child.id();
    let mut app = ApplicationProcess::from(child);
    assert!(
        !app.cancel_namespace(),
        "namespace cancellation cannot abandon a host child"
    );
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        assert!(Instant::now() < deadline, "host reap deadline");
        match poll(&mut f, &mut app) {
            Some(ApplicationProcessEvent::Exited(status)) => {
                assert!(status.success());
                break;
            }
            None => {}
            _ => panic!("ordinary host process must retain its ordinary exit semantics"),
        }
    }
    assert!(poll(&mut f, &mut app).is_none());
    assert_eq!(
        app.host_mut().unwrap().id(),
        pid,
        "Session retains the host child for process-group cleanup"
    );
    assert_eq!(f.service.occupied(), 0);
}

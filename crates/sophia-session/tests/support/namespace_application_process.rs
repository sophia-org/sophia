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

#[test]
fn namespace_provenance_is_captured_before_exec_and_before_activation() {
    for protected in [false, true] {
        let profile = if protected {
            sophia_protocol::NamespaceProfile::Confined
        } else {
            sophia_protocol::NamespaceProfile::ClassicShared
        };
        let mut f = Fixture::with_profile(profile);
        let mut request = f.request();
        if !protected {
            request.launch.protection_domain = None;
            std::fs::write(
                f.root.join("shared"),
                "namespace-recipe 1\nmount portal roles/portal\n",
            )
            .unwrap();
            let mut policy = request.recipe.policy().clone();
            policy.profile = NamespaceRecipeProfile::SharedX;
            policy.source = Some("shared".into());
            policy.developer_launch = false;
            request.recipe = LoadedNamespaceRecipe::load(&policy, &f.root)
                .unwrap()
                .into();
            let mut endpoint = request.approved.remove("inspection").unwrap();
            endpoint.service.interface = "portal-client".into();
            request.approved.insert("portal".into(), endpoint);
        }
        let mut pending = f
            .service
            .start(
                request,
                &mut f.auth.coordinator,
                f.wake.notifier(),
                Instant::now(),
            )
            .unwrap();
        // Retain the real gate temporarily to observe the pre-exec boundary.
        // Startup still uses the production worker, channel and supervisor.
        let mut gate = pending.gate.take().unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let pid = loop {
            assert!(Instant::now() < deadline, "gated namespace fork deadline");
            f.auth.tick();
            assert!(
                pending
                    .poll(&mut f.auth.coordinator, f.wake.notifier(), Instant::now())
                    .unwrap()
                    .is_none()
            );
            if let Some(pid) = gate.poll().unwrap() {
                break pid;
            }
        };
        assert!(pending.process_identity.is_none());
        let expected = crate::launch_origin::read_process(pid).unwrap().identity;
        pending.gate = Some(gate);
        let mut app =
            ApplicationProcess::namespace(pending, NamespaceApplicationServices::default());
        assert!(
            app.poll(&mut f.auth.coordinator, f.wake.notifier(), Instant::now())
                .is_none(),
            "pre-exec provenance must precede service activation"
        );
        assert_eq!(
            app.launch_identity(),
            Some(expected),
            "namespace launch must retain the gated process identity before activation"
        );
        assert!(app.cancel_namespace());
        f.reclaimed();
    }
}

#[test]
fn revoked_launch_cancels_a_waiting_exec_handshake() {
    let mut f = Fixture::new();
    let mut pending = f.start();
    let registration = pending.registration.as_ref().unwrap().registration();
    let mut gate = pending.gate.take().unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let pid = loop {
        assert!(
            Instant::now() < deadline,
            "revoked fork observation deadline"
        );
        f.auth.tick();
        assert!(
            pending
                .poll(&mut f.auth.coordinator, f.wake.notifier(), Instant::now())
                .unwrap()
                .is_none()
        );
        if let Some(pid) = gate.poll().unwrap() {
            break pid;
        }
    };
    pending.gate = Some(gate);
    f.auth
        .coordinator
        .revoke(registration, Instant::now())
        .unwrap();
    assert!(
        pending
            .poll(&mut f.auth.coordinator, f.wake.notifier(), Instant::now())
            .is_err()
    );
    assert!(
        pending.process_identity.is_none(),
        "revoked authority must not release the provenance handshake"
    );
    f.reclaimed();
    assert!(
        crate::launch_origin::read_process(pid).is_none(),
        "cancelled fork must be reaped"
    );
}

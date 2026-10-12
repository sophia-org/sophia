//! A legacy X-profile process need not implement or retain the optional 9P
//! capability. Supplied X sockets/cookies; no X setup or pixels are claimed.
use super::*;

#[test]
fn optional_x_protocol_does_not_bypass_revocation_before_exec() {
    for namespace_loss in [false, true] {
        let mut f = Fixture::with_profile(NamespaceProfile::ClassicShared);
        let (target, _listener) = target(&f, None);
        let set =
            ApplicationRecipeSet::load(&policy(NamespaceRecipeProfile::SharedX), &f.root).unwrap();
        let command = command();
        let resolved = target
            .resolve(
                set.application("registered:browser", &command)
                    .unwrap()
                    .unwrap(),
                &command,
            )
            .unwrap();
        let mut pending = f
            .service
            .start(
                resolved.request,
                &mut f.auth.coordinator,
                f.wake.notifier(),
                Instant::now(),
            )
            .unwrap();
        let registration = pending.registration.as_ref().unwrap().registration();
        let mut gate = pending.gate.take().unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let pid = loop {
            assert!(
                Instant::now() < deadline,
                "X fork must reach held exec gate"
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
        if namespace_loss {
            f.service
                .registry
                .lock()
                .unwrap()
                .revoke_namespace(NamespaceId::from_raw(2))
                .unwrap();
        } else {
            f.auth
                .coordinator
                .revoke(registration, Instant::now())
                .unwrap();
        }
        assert!(
            pending
                .poll(&mut f.auth.coordinator, f.wake.notifier(), Instant::now())
                .is_err(),
            "optional X protocol must not bypass revocation before exec"
        );
        assert!(
            pending.process_identity.is_none(),
            "revoked namespace must not release the exec gate"
        );
        f.reclaimed();
        assert!(crate::launch_origin::read_process(pid).is_none());
    }
}

#[test]
fn optional_protocol_loss_fences_retained_authority_but_not_namespace_retirement() {
    for group in [None, Some(7)] {
        let mut f = Fixture::with_profile(if group.is_some() {
            NamespaceProfile::Confined
        } else {
            NamespaceProfile::ClassicShared
        });
        let (target, _listener) = target(&f, group);
        let cookie = f.root.join("cookie");
        std::fs::write(&cookie, "wait").unwrap();
        let profile = group.map_or(NamespaceRecipeProfile::SharedX, |group| {
            NamespaceRecipeProfile::ConfinedX { group }
        });
        let set = ApplicationRecipeSet::load(&policy(profile), &f.root).unwrap();
        let mut command = command();
        command.executable = "/usr/bin/bash".into();
        command.arguments = vec!["--noprofile".into(), "--norc".into(), "-c".into(),
            "while [ \"$(cat \"$XAUTHORITY\")\" != go ]; do sleep 0.01; done; fd=$SOPHIA_LAUNCH_AUTH_FD; eval \"exec $fd>&-\"; sleep 30".into()];
        let resolved = target
            .resolve(
                set.application("registered:browser", &command)
                    .unwrap()
                    .unwrap(),
                &command,
            )
            .unwrap();
        let mut pending = f
            .service
            .start(
                resolved.request,
                &mut f.auth.coordinator,
                f.wake.notifier(),
                Instant::now(),
            )
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut running = loop {
            assert!(
                Instant::now() < deadline,
                "X application activation deadline"
            );
            f.auth.tick();
            if let Some(running) = pending
                .poll(&mut f.auth.coordinator, f.wake.notifier(), Instant::now())
                .unwrap()
            {
                break running;
            }
        };
        let mut endpoints = running.take_endpoints();
        assert!(!endpoints.is_empty());
        let grant = endpoints[0].grant.clone();
        assert!(grant.is_live());
        let server = sophia_9p::unix::Server::new(
            sophia_factotum::export::FactotumExport::new(sophia_factotum::proto::Settings {
                owner: "fixture".into(),
                pam_service: "fixture".into(),
            }),
            sophia_9p::Limits::default(),
        )
        .unwrap();
        let mut transport = endpoints[0]
            .factory
            .create(sophia_9p::ConnectionId(1), Instant::now(), server.wake())
            .unwrap();
        std::fs::write(&cookie, "go").unwrap();
        while running.protocol_available() {
            assert!(
                Instant::now() < deadline,
                "optional protocol revocation deadline"
            );
            f.auth.tick();
            assert!(running.poll(&f.auth.coordinator).unwrap().is_none());
        }
        assert!(
            !grant.is_live(),
            "unused protocol loss must revoke retained endpoint grants"
        );
        assert_eq!(
            transport.poll(Instant::now()),
            Err(sophia_9p::Errno::ESTALE),
            "unused protocol loss must fence retained authentication"
        );
        let pid = running.process_id().unwrap();
        assert!(
            crate::launch_origin::read_process(pid).is_some(),
            "X application must remain alive after protocol loss"
        );
        f.service
            .registry
            .lock()
            .unwrap()
            .revoke_namespace(NamespaceId::from_raw(2))
            .unwrap();
        assert_eq!(
            running.poll(&f.auth.coordinator).unwrap_err(),
            "application namespace retired",
            "optional protocol loss must not mask X namespace retirement"
        );
        drop(running);
        f.reclaimed();
        assert!(
            crate::launch_origin::read_process(pid).is_none(),
            "retired X application must be reaped"
        );
    }
}

#[test]
fn closing_unused_protocol_channel_does_not_terminate_an_x_application() {
    for (group, after_activation) in [
        (None, false),
        (Some(7), false),
        (None, true),
        (Some(7), true),
    ] {
        let profile = if group.is_some() {
            NamespaceProfile::Confined
        } else {
            NamespaceProfile::ClassicShared
        };
        let mut f = Fixture::with_profile(profile);
        let (target, _listener) = target(&f, group);
        // The cookie is a supplied fixture file, not an X authentication
        // witness. Its read-only child view supplies a deterministic barrier.
        let cookie = f.root.join("cookie");
        std::fs::write(&cookie, "wait").unwrap();
        let profile = group.map_or(NamespaceRecipeProfile::SharedX, |group| {
            NamespaceRecipeProfile::ConfinedX { group }
        });
        let set = ApplicationRecipeSet::load(&policy(profile), &f.root).unwrap();
        let mut command = command();
        command.executable = "/usr/bin/bash".into();
        command.arguments = vec![
            "--noprofile".into(),
            "--norc".into(),
            "-c".into(),
            format!(
                "{} fd=$SOPHIA_LAUNCH_AUTH_FD; [[ $fd =~ ^[0-9]+$ ]] || exit 91; eval \"exec $fd>&-\"; sleep 0.5; exit 17",
                if after_activation {
                    "while [ \"$(cat \"$XAUTHORITY\")\" != go ]; do sleep 0.01; done;"
                } else {
                    ""
                }
            ),
        ];
        let resolved = target
            .resolve(
                set.application("registered:browser", &command)
                    .unwrap()
                    .unwrap(),
                &command,
            )
            .unwrap();
        let mut app = resolved
            .start(
                &f.service,
                &mut f.auth.coordinator,
                f.wake.notifier(),
                Instant::now(),
                crate::diagnostics::application::LaunchContext {
                    source: crate::diagnostics::application::LaunchSource::Catalog,
                    transaction: Some(33),
                },
            )
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            assert!(Instant::now() < deadline, "optional protocol exit deadline");
            f.auth.tick();
            match app.poll(&mut f.auth.coordinator, f.wake.notifier(), Instant::now()) {
                Some(ApplicationProcessEvent::Started { .. }) => {
                    if after_activation {
                        std::fs::write(&cookie, "go").unwrap();
                    }
                }
                Some(ApplicationProcessEvent::Exited(status)) => {
                    assert_eq!(
                        status.code(),
                        Some(17),
                        "X process keeps its own exit status after optional protocol loss"
                    );
                    break;
                }
                Some(ApplicationProcessEvent::Failed(reason)) => panic!(
                    "closing unused protocol channel must not terminate an X application: {reason}"
                ),
                None => std::thread::sleep(Duration::from_millis(1)),
            }
        }
        f.reclaimed();
    }
}

//! Diagnostics follow the real recipe scheduler and its retirement workers.
use super::*;
use crate::diagnostics::{
    Retention, Store,
    application::{ApplicationCapture, LaunchContext, LaunchSource},
};

#[test]
fn short_lived_namespace_applications_keep_exit_status_without_claiming_activation() {
    for (protected, code) in [(true, 0), (true, 17), (false, 0), (false, 17)] {
        let mut f = Fixture::with_profile(if protected {
            sophia_protocol::NamespaceProfile::Confined
        } else {
            sophia_protocol::NamespaceProfile::ClassicShared
        });
        let store = Store::open(&f.root.join("diagnostics"), Retention::default()).unwrap();
        let record = store.begin("early-exit", std::process::id(), "").unwrap();
        let capture = ApplicationCapture::start(&record.path).unwrap();
        capture.set_enabled(true);
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
        request.launch.program = "/usr/bin/sh".into();
        request.launch.args = vec![
            "-c".into(),
            format!("printf early-stderr >&2; exit {code}").into(),
        ];
        request.diagnostic = capture.prepare(
            &request.launch.program,
            LaunchContext {
                source: LaunchSource::Catalog,
                transaction: Some(30),
            },
        );
        let mut pending = f
            .service
            .start(
                request,
                &mut f.auth.coordinator,
                f.wake.notifier(),
                Instant::now(),
            )
            .unwrap();
        let registration = pending.registration.as_ref().unwrap().registration();
        let deadline = Instant::now() + Duration::from_secs(10);
        // Stop the owner before it takes the spawn result; the real background
        // spawn and child run to completion without an activation poll.
        while f.auth.coordinator.phase(registration) != Some(Phase::Spawning) {
            f.auth.tick();
            assert!(
                pending
                    .poll(&mut f.auth.coordinator, f.wake.notifier(), Instant::now())
                    .unwrap()
                    .is_none()
            );
            assert!(Instant::now() < deadline, "early-exit spawn deadline");
        }
        // Complete only the provenance handshake, keeping the activation
        // owner paused while the real child exits.
        while pending.process_identity.is_none() {
            pending.capture_provenance().unwrap();
            assert!(Instant::now() < deadline, "early-exit provenance deadline");
            std::thread::sleep(Duration::from_millis(1));
        }
        loop {
            let records = store.application_records(&record.id).unwrap();
            if let Some(metadata) = records.launches.get(&1)
                && metadata.contains("spawn=spawned")
                && metadata.contains("eof=true")
            {
                let pid = metadata
                    .split_whitespace()
                    .find_map(|field| field.strip_prefix("pid="))
                    .unwrap();
                let stat = std::fs::read_to_string(format!("/proc/{pid}/stat"));
                if stat
                    .as_ref()
                    .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound)
                    || stat.as_ref().is_ok_and(|stat| {
                        stat.rsplit_once(')')
                            .is_some_and(|(_, rest)| rest.split_whitespace().next() == Some("Z"))
                    })
                {
                    break;
                }
            }
            assert!(
                Instant::now() < deadline,
                "early child exit before activation"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
        let mut app =
            ApplicationProcess::namespace(pending, NamespaceApplicationServices::default());
        loop {
            match poll(&mut f, &mut app) {
                Some(ApplicationProcessEvent::Exited(status)) => {
                    assert_eq!(
                        status.code(),
                        Some(code),
                        "early exit must retain the actual process status"
                    );
                    break;
                }
                Some(ApplicationProcessEvent::Started { .. }) => {
                    panic!("an exited client must never claim Active custody")
                }
                Some(ApplicationProcessEvent::Failed(reason)) => {
                    panic!("early exit must not become a launch failure: {reason}")
                }
                None => {
                    assert!(Instant::now() < deadline);
                    std::thread::sleep(Duration::from_millis(1));
                }
            }
        }
        assert!(poll(&mut f, &mut app).is_none());
        f.reclaimed();
        assert_eq!(f.service.registry.lock().unwrap().endpoint_grant_count(), 0);
        loop {
            if store
                .application_records(&record.id)
                .unwrap()
                .launches
                .get(&1)
                .is_some_and(|m| {
                    m.contains("exit_observed=true") && m.contains(&format!("exit_code={code}"))
                })
            {
                break;
            }
            assert!(Instant::now() < deadline, "early exit diagnostics settle");
            std::thread::sleep(Duration::from_millis(2));
        }
        drop(capture);
    }
}

#[test]
fn diagnostics_settle_on_exit_cancellation_and_failed_namespace_startup() {
    for scenario in [
        "exit",
        "cancel",
        "prepare-cancel",
        "dispatch-failure",
        "spawn-failure",
    ] {
        let mut f = Fixture::new();
        let store = Store::open(&f.root.join("diagnostics"), Retention::default()).unwrap();
        let record = store.begin("namespace", std::process::id(), "").unwrap();
        let capture = ApplicationCapture::start(&record.path).unwrap();
        capture.set_enabled(true);
        let mut request = f.request();
        request.launch.program = if scenario == "spawn-failure" {
            "/missing-namespace-diagnostic-app"
        } else {
            "/usr/bin/sh"
        }
        .into();
        request.launch.args = vec![
            "-c".into(),
            if scenario == "exit" {
                "printf namespace-stderr >&2; sleep 0.3; exit 17"
            } else {
                "printf namespace-stderr >&2; sleep 30"
            }
            .into(),
        ];
        request.diagnostic = capture.prepare(
            &request.launch.program,
            LaunchContext {
                source: LaunchSource::Catalog,
                transaction: Some(29),
            },
        );
        // The scheduler owns this reserved value even if a caller tried to remove it.
        request.launch = request.launch.env_remove(NAMESPACE_DIRECTORY_ENV);
        let (mut app, _service) =
            application_request(&mut f, request, scenario != "dispatch-failure");
        let deadline = Instant::now() + Duration::from_secs(10);
        if scenario == "prepare-cancel" {
            assert!(app.cancel_namespace());
        } else {
            loop {
                assert!(
                    Instant::now() < deadline,
                    "{scenario}: diagnostic application deadline"
                );
                match poll(&mut f, &mut app) {
                    Some(ApplicationProcessEvent::Started { .. }) if scenario == "cancel" => {
                        // Wait for the child write, not an arbitrary timer, before cancellation.
                        while store
                            .application_records(&record.id)
                            .unwrap()
                            .chunks
                            .is_empty()
                        {
                            assert!(
                                Instant::now() < deadline,
                                "child stderr before cancellation"
                            );
                            std::thread::sleep(Duration::from_millis(2));
                        }
                        assert!(app.cancel_namespace());
                        break;
                    }
                    Some(ApplicationProcessEvent::Started { .. }) => assert_eq!(scenario, "exit"),
                    Some(ApplicationProcessEvent::Exited(status)) => {
                        assert_eq!(scenario, "exit");
                        assert_eq!(status.code(), Some(17));
                        break;
                    }
                    Some(ApplicationProcessEvent::Failed(reason)) => {
                        assert!(
                            matches!(scenario, "dispatch-failure" | "spawn-failure"),
                            "{scenario}: {reason}"
                        );
                        break;
                    }
                    None => std::thread::sleep(Duration::from_millis(1)),
                }
            }
        }
        f.reclaimed();
        let spawned = matches!(scenario, "exit" | "cancel" | "dispatch-failure");
        let records = loop {
            let records = store.application_records(&record.id).unwrap();
            if records.launches.get(&1).is_some_and(|metadata| {
                metadata.contains("eof=true")
                    && if spawned {
                        metadata.contains("spawn=spawned")
                            && metadata.contains("exit_observed=true")
                    } else {
                        metadata.contains("spawn=failed")
                    }
            }) {
                break records;
            }
            assert!(
                Instant::now() < deadline,
                "{scenario}: namespace diagnostics must settle after retirement"
            );
            std::thread::sleep(Duration::from_millis(2));
        };
        assert!(records.launches[&1].contains("transaction=29"));
        if matches!(scenario, "exit" | "cancel") {
            let bytes: Vec<u8> = records
                .chunks
                .iter()
                .flat_map(|(_, _, bytes)| bytes.iter().copied())
                .collect();
            assert_eq!(
                bytes, b"namespace-stderr",
                "namespace lifecycle must retain exact stderr bytes"
            );
        }
        drop(capture);
    }
}

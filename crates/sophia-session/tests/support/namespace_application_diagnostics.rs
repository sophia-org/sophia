//! Diagnostics follow the real recipe scheduler and its retirement workers.
use super::*;
use crate::diagnostics::{
    Retention, Store,
    application::{ApplicationCapture, LaunchContext, LaunchSource},
};

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

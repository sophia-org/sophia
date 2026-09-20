#![cfg(test)]

//! Who the frontend is told may inject.
//!
//! The decision itself is Session's issuance, exercised in full by the
//! private-input tests beside these; what is proved here is the translation
//! the frontend receives, and that no client can reach the policy before it
//! has a runtime to issue from.
//!
//! ONE SERVICE BETWEEN THEM, DELIBERATELY. A service started here runs
//! alongside every other test in this binary, and two of the deferred-control
//! tests miss their deadlines when more start beside them. What that leaves
//! uncovered -- a service that issues nothing to anyone -- is the
//! `xtest_disabled` wire case, which proves the same refusal where a client
//! actually meets it.

use super::injection::PrivateInputInjectionPolicy;
use super::{
    PrivateInputConfig, PrivateInputGrantPolicy, PrivateInputInstanceCookie,
    PrivateInputLifetimeOwner, PrivateInputReadiness, PrivateInputService,
};
use sophia_input_authority::{InstanceId, SeatBinding};
use sophia_protocol::{
    ClientAdmissionContext, ClientAdmissionId, ClientAuthProvenance, ClientAuthenticationMethod,
    DeviceId, NamespaceCapabilities, NamespaceContext, NamespaceId, NamespaceProfile, OutputId,
    OutputTopologyEntry, OutputTopologySnapshot, Rect, SeatId, Size,
};
use sophia_x_authority::{XServerFrontendInjectionError, XServerFrontendInjectionPolicy};
use std::num::NonZeroUsize;
use std::path::Path;
use std::time::Duration;

const WAIT: Duration = Duration::from_secs(8);
const NAMESPACE: u64 = 733;

/// A context shaped like an admitted connection's, belonging to none.
///
/// Every refusal these tests ask for is decided before the admission is
/// looked up or is decided by its absence, so a context that names no live
/// connection is exactly the input under test.
fn stranger() -> ClientAdmissionContext {
    ClientAdmissionContext::new(
        ClientAdmissionId::from_raw(9),
        NamespaceContext::new(
            NamespaceId::from_raw(NAMESPACE),
            NamespaceProfile::Confined,
            NamespaceCapabilities::NONE,
        )
        .unwrap(),
        ClientAuthProvenance::new(ClientAuthenticationMethod::TrustedLocal, 1).unwrap(),
    )
    .unwrap()
}

fn config(socket: &Path, grants: PrivateInputGrantPolicy) -> PrivateInputConfig {
    let instance = InstanceId::new(733);
    let output = OutputId::from_raw(1);
    let size = Size {
        width: 320,
        height: 240,
    };
    PrivateInputConfig {
        socket_path: socket.to_owned(),
        namespace: NamespaceId::from_raw(NAMESPACE),
        session_generation: 1,
        profile: NamespaceProfile::Confined,
        capabilities: NamespaceCapabilities::NONE,
        frame_clock: sophia_engine::DeterministicFrameClock::new(1, 16),
        binding: SeatBinding::new(instance, SeatId::from_raw(1)),
        cookie: PrivateInputInstanceCookie {
            instance,
            cookie: [0x5b; 32],
        },
        grants,
        max_concurrent_clients: NonZeroUsize::new(4).unwrap(),
        input_capacity: NonZeroUsize::new(8).unwrap(),
        advertised_buttons: 9,
        output_topology: OutputTopologySnapshot {
            generation: 1,
            primary: output,
            outputs: vec![OutputTopologyEntry {
                output,
                logical: Rect {
                    x: 0,
                    y: 0,
                    width: size.width,
                    height: size.height,
                },
                pixel_size: size,
                scale: 1,
                refresh_millihz: 60_000,
                timing: None,
            }],
        },
    }
}

#[test]
fn a_policy_with_no_runtime_offers_no_injection() {
    let policy = PrivateInputInjectionPolicy::new();
    assert_eq!(
        policy.issue(stranger(), DeviceId::from_raw(1)).err(),
        Some(XServerFrontendInjectionError::Unavailable),
        "an unpublished policy states an absence, not a denial"
    );
}

#[test]
fn a_service_that_has_ended_offers_no_injection() {
    let directory =
        std::env::temp_dir().join(format!("private-injection-ended-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(&directory).unwrap();
    let policy = PrivateInputInjectionPolicy::new();
    {
        let lifetime = PrivateInputLifetimeOwner::reserved();
        let handle = PrivateInputService::start(
            &lifetime,
            config(
                &directory.join("input.sock"),
                PrivateInputGrantPolicy::EnabledWithVerifiedEvidence,
            ),
        )
        .unwrap();
        assert_eq!(
            handle.await_ready(WAIT).unwrap(),
            PrivateInputReadiness::Ready
        );
        assert!(policy.publish(&handle.runtime));
        assert!(
            !policy.publish(&handle.runtime),
            "a second runtime would answer for the first's clients"
        );
        // Published and live: this admission is refused on its own terms.
        assert_eq!(
            policy.issue(stranger(), DeviceId::from_raw(1)).err(),
            Some(XServerFrontendInjectionError::Denied)
        );
        let outcome = handle.stop();
        assert!(outcome.failure.is_none(), "{outcome:?}");
    }
    // The runtime is gone, so there is nothing to issue from and nothing to
    // decide with. An absence, again, rather than a denial.
    assert_eq!(
        policy.issue(stranger(), DeviceId::from_raw(1)).err(),
        Some(XServerFrontendInjectionError::Unavailable)
    );
    std::fs::remove_dir_all(&directory).unwrap();
}

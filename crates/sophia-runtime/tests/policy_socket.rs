#![cfg(target_os = "linux")]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use sophia_runtime::{
    PolicyPeerIdentity, PolicyRole, PolicyRoleEndpoint, PolicyRoleEndpointError,
    ProtectionBackendKind, ProtectionDomainEvidence, ProtectionDomainRole, SOPHIA_WM_9P_SOCKET_ENV,
};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(1);

#[test]
fn endpoint_is_owner_only_and_admits_only_the_supervised_peer() {
    let directory = unique_directory("admission");
    let peer = current_peer();
    let mut endpoint = PolicyRoleEndpoint::bind(&directory, peer).unwrap();
    assert_eq!(SOPHIA_WM_9P_SOCKET_ENV, "SOPHIA_WM_9P_SOCKET");
    assert_eq!(
        fs::metadata(&directory).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert_eq!(
        fs::metadata(endpoint.socket_path())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );

    let client = UnixStream::connect(endpoint.socket_path()).unwrap();
    let accepted = endpoint.accept_expected().unwrap();
    assert_eq!(endpoint.active_peer(), Some(peer));
    assert_eq!(
        endpoint.accept_expected().unwrap_err(),
        PolicyRoleEndpointError::PeerAlreadyActive
    );
    drop((client, accepted));
    endpoint.release_peer(peer).unwrap();
    drop(endpoint);
    assert!(!directory.exists());
}

#[test]
fn broker_endpoint_uses_the_same_owner_only_admission_boundary() {
    let directory = unique_directory("broker-admission");
    let endpoint = PolicyRoleEndpoint::bind_role_for_supervised_uid(
        &directory,
        PolicyRole::Broker,
        current_peer().uid,
    )
    .unwrap();
    assert_eq!(endpoint.socket_path(), directory.join("broker.sock"));
    drop(endpoint);
    assert!(!directory.exists());
}

/// A metadata-bearing role may not be handed a peer identity at bind time.
///
/// Naming a PID up front is admission on supervision alone, so refusing it here
/// is the same rule as `authorize_supervised_pid`, applied at the other door.
/// Without this the constructor stayed a way in for exactly what the admission
/// call rejects.
#[test]
fn a_metadata_bearing_role_cannot_bind_against_a_bare_peer_identity() {
    for role in [PolicyRole::Broker, PolicyRole::Shell] {
        let directory = unique_directory("metadata-bind");
        assert_eq!(
            PolicyRoleEndpoint::bind_role(&directory, role, current_peer()).err(),
            Some(PolicyRoleEndpointError::ProtectionDomainRequired {
                role,
                required: role.domain_role(),
            })
        );
        // A refused bind leaves nothing behind to collide with the retry.
        assert!(!directory.exists());
    }
}

/// The gap this rule closes: a supervisor that builds no protection domain used
/// to admit a metadata-bearing peer with no boundary and no complaint, because
/// the forbidden-composition check only fires for a caller that builds a domain.
#[test]
fn a_metadata_bearing_role_refuses_a_bare_supervised_pid() {
    for role in [PolicyRole::Broker, PolicyRole::Shell] {
        let directory = unique_directory("metadata-pid");
        let mut endpoint =
            PolicyRoleEndpoint::bind_role_for_supervised_uid(&directory, role, current_peer().uid)
                .unwrap();
        assert_eq!(
            endpoint.authorize_supervised_pid(current_peer().pid).err(),
            Some(PolicyRoleEndpointError::ProtectionDomainRequired {
                role,
                required: role.domain_role(),
            })
        );
        // Refused authorization leaves the endpoint unarmed rather than half-armed.
        assert_eq!(
            endpoint
                .accept_expected_timeout(Duration::from_millis(20))
                .err(),
            Some(PolicyRoleEndpointError::PeerNotAuthorized)
        );
    }
}

/// Evidence admits only for the role the domain actually carries. A blind
/// spatial-policy domain cannot stand in for a broker's.
#[test]
fn a_metadata_bearing_role_admits_only_evidence_carrying_its_role() {
    let directory = unique_directory("metadata-evidence");
    let peer = current_peer();
    let mut endpoint =
        PolicyRoleEndpoint::bind_role_for_supervised_uid(&directory, PolicyRole::Broker, peer.uid)
            .unwrap();

    assert_eq!(
        endpoint
            .authorize_protected_peer(&evidence(peer.pid, [ProtectionDomainRole::SpatialPolicy]))
            .err(),
        Some(PolicyRoleEndpointError::ProtectionRoleMissing {
            required: ProtectionDomainRole::MetadataBroker,
            observed: [ProtectionDomainRole::SpatialPolicy].into_iter().collect(),
        })
    );

    endpoint
        .authorize_protected_peer(&evidence(peer.pid, [ProtectionDomainRole::MetadataBroker]))
        .unwrap();
    let client = UnixStream::connect(endpoint.socket_path()).unwrap();
    let accepted = endpoint.accept_expected().unwrap();
    assert_eq!(endpoint.active_peer(), Some(peer));
    drop((client, accepted));
}

/// The blind roles keep admitting on supervision alone.
///
/// Requiring a domain everywhere is a separate decision that has to answer for
/// hosts with no `bwrap`; this test is what keeps it from arriving as a side
/// effect of the metadata-bearing rule.
#[test]
fn blind_roles_still_admit_a_supervised_pid_without_a_domain() {
    for role in [PolicyRole::Wm, PolicyRole::Output] {
        assert!(!role.is_metadata_bearing());
        let directory = unique_directory("blind-pid");
        let peer = current_peer();
        let mut endpoint =
            PolicyRoleEndpoint::bind_role_for_supervised_uid(&directory, role, peer.uid).unwrap();
        endpoint.authorize_supervised_pid(peer.pid).unwrap();
        let client = UnixStream::connect(endpoint.socket_path()).unwrap();
        let accepted = endpoint.accept_expected().unwrap();
        assert_eq!(endpoint.active_peer(), Some(peer));
        drop((client, accepted));
    }
}

/// One domain holding both roles admits on both endpoints, which is how a WM
/// granted the output authority connects twice without widening either role.
#[test]
fn one_domain_admits_every_role_it_carries() {
    let peer = current_peer();
    let both = evidence(
        peer.pid,
        [
            ProtectionDomainRole::SpatialPolicy,
            ProtectionDomainRole::OutputAuthority,
        ],
    );
    for role in [PolicyRole::Wm, PolicyRole::Output] {
        let directory = unique_directory("shared-domain");
        let mut endpoint =
            PolicyRoleEndpoint::bind_role_for_supervised_uid(&directory, role, peer.uid).unwrap();
        endpoint.authorize_protected_peer(&both).unwrap();
    }
}

#[test]
fn credential_mismatch_fails_without_claiming_the_role() {
    let directory = unique_directory("credentials");
    let mut expected = current_peer();
    expected.pid = expected.pid.saturating_add(1);
    let mut endpoint = PolicyRoleEndpoint::bind(&directory, expected).unwrap();
    let _client = UnixStream::connect(endpoint.socket_path()).unwrap();

    assert!(matches!(
        endpoint.accept_expected(),
        Err(PolicyRoleEndpointError::UnauthorizedPeer { expected: denied, actual })
            if denied == expected && actual == current_peer()
    ));
    assert_eq!(endpoint.active_peer(), None);
}

#[test]
fn supervised_endpoint_requires_authorization_before_accepting_the_exact_child() {
    let directory = unique_directory("supervised");
    let peer = current_peer();
    let mut endpoint = PolicyRoleEndpoint::bind_for_supervised_uid(&directory, peer.uid).unwrap();
    let client = UnixStream::connect(endpoint.socket_path()).unwrap();

    assert_eq!(
        endpoint.accept_expected().unwrap_err(),
        PolicyRoleEndpointError::PeerNotAuthorized
    );
    endpoint.authorize_supervised_pid(peer.pid).unwrap();
    let accepted = endpoint.accept_expected().unwrap();
    assert_eq!(endpoint.active_peer(), Some(peer));
    drop((client, accepted));
}

#[test]
fn supervised_endpoint_rejects_a_process_other_than_the_authorized_child() {
    let directory = unique_directory("wrong-supervised-peer");
    let peer = current_peer();
    let mut endpoint = PolicyRoleEndpoint::bind_for_supervised_uid(&directory, peer.uid).unwrap();
    endpoint
        .authorize_supervised_pid(peer.pid.saturating_add(1))
        .unwrap();
    let _client = UnixStream::connect(endpoint.socket_path()).unwrap();

    assert!(matches!(
        endpoint.accept_expected(),
        Err(PolicyRoleEndpointError::UnauthorizedPeer { expected, actual })
            if expected.pid == peer.pid.saturating_add(1) && actual == peer
    ));
    assert_eq!(endpoint.active_peer(), None);
}

#[test]
fn endpoint_never_reuses_an_existing_directory() {
    let directory = unique_directory("existing");
    fs::create_dir(&directory).unwrap();
    assert!(matches!(
        PolicyRoleEndpoint::bind(&directory, current_peer()),
        Err(PolicyRoleEndpointError::PathAlreadyExists)
    ));
    fs::remove_dir(directory).unwrap();
}

#[test]
fn expected_peer_accept_is_bounded_when_no_client_connects() {
    let directory = unique_directory("accept-timeout");
    let mut endpoint = PolicyRoleEndpoint::bind(&directory, current_peer()).unwrap();
    let started = Instant::now();

    assert_eq!(
        endpoint
            .accept_expected_timeout(Duration::from_millis(20))
            .unwrap_err(),
        PolicyRoleEndpointError::AcceptTimedOut
    );
    assert!(started.elapsed() < Duration::from_secs(1));
    assert_eq!(endpoint.active_peer(), None);
}

fn evidence(
    peer_pid: u32,
    roles: impl IntoIterator<Item = ProtectionDomainRole>,
) -> ProtectionDomainEvidence {
    ProtectionDomainEvidence {
        backend: ProtectionBackendKind::Bubblewrap,
        supervisor_pid: std::process::id(),
        peer_pid,
        roles: roles.into_iter().collect(),
    }
}

fn current_peer() -> PolicyPeerIdentity {
    PolicyPeerIdentity {
        uid: rustix::process::geteuid().as_raw(),
        pid: std::process::id(),
    }
}

fn unique_directory(label: &str) -> std::path::PathBuf {
    let serial = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "sophia-policy-{label}-{}-{serial}",
        std::process::id()
    ))
}

#[test]
fn every_role_has_its_own_socket_and_environment_variable() {
    // A role is an interface family and an admission boundary at once. Two roles
    // sharing a socket would let one authority's peer connect where another's was
    // authorized, which is the conflation the split exists to prevent -- and a
    // shared env var would send it there by accident rather than by attack.
    let roles = [
        PolicyRole::Wm,
        PolicyRole::Shell,
        PolicyRole::Broker,
        PolicyRole::Output,
    ];

    let mut names = roles.map(PolicyRole::socket_file_name).to_vec();
    names.sort_unstable();
    names.dedup();
    assert_eq!(
        names.len(),
        roles.len(),
        "socket file names must be distinct"
    );

    let mut envs = roles.map(PolicyRole::socket_env).to_vec();
    envs.sort_unstable();
    envs.dedup();
    assert_eq!(envs.len(), roles.len(), "socket env vars must be distinct");
    assert_eq!(PolicyRole::Wm.socket_env(), "SOPHIA_WM_9P_SOCKET");
    assert_eq!(PolicyRole::Shell.socket_env(), "SOPHIA_SHELL_9P_SOCKET");

    assert_eq!(PolicyRole::Broker.socket_file_name(), "broker.sock");
    assert_eq!(PolicyRole::Broker.socket_env(), "SOPHIA_BROKER_SOCKET");
    assert_eq!(PolicyRole::Output.socket_file_name(), "output.sock");
    assert_eq!(PolicyRole::Output.socket_env(), "SOPHIA_OUTPUT_SOCKET");
}

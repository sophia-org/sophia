//! Explicit per-epoch SFT permission must not widen ordinary role admission.
use crate::shell_transport::{
    ShellComponentTransport, ShellContentAdmissionPolicy, ShellTransportError,
};
use crate::{ContentEpochRegistry, ContentStoreProfile};
use sophia_protocol::shell_files::SHELL_FRAME_CAPABILITY;
use sophia_protocol::*;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

const CAPS: u64 = 1 | (1 << 7) | (1 << 15) | SHELL_FRAME_CAPABILITY;
const GRANTED: ShellContentAdmissionPolicy = ShellContentAdmissionPolicy::Granted {
    discrete_input: false,
};

struct Owner {
    state: ShellComponentTransport,
    epochs: ContentEpochRegistry,
    path: PathBuf,
}

impl Owner {
    fn new(profile: ContentStoreProfile) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "sft-negotiation-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let mut state = ShellComponentTransport::bind_for_supervised_uid(
            &path,
            rustix::process::geteuid().as_raw(),
        )
        .unwrap();
        state
            .authorize_authenticated_peer(
                &crate::ProtectionDomainEvidence {
                    backend: crate::ProtectionBackendKind::Bubblewrap,
                    supervisor_pid: std::process::id(),
                    peer_pid: std::process::id(),
                    roles: [crate::ProtectionDomainRole::MetadataShell]
                        .into_iter()
                        .collect(),
                },
                Box::new(crate::image_test_authority::Factory),
            )
            .unwrap();
        let (discovery, limits, formats, bounds) =
            crate::shell_transport::owned_copy::wire_tests::config();
        state
            .configure_image_channel(discovery, limits, formats, bounds)
            .unwrap();
        let mut epochs = ContentEpochRegistry::new(64 << 20).unwrap();
        state
            .reserve_content_with_profile(
                &mut epochs,
                ContentLimits::prototype(ContentGrant {
                    connection_epoch: 1,
                    content_grant_epoch: 1,
                }),
                profile,
            )
            .unwrap();
        Self {
            state,
            epochs,
            path,
        }
    }

    fn select(
        &mut self,
        revision: u16,
        caps: u64,
        policy: ShellContentAdmissionPolicy,
    ) -> Result<ShellV1ServerWelcome, ShellTransportError> {
        self.state
            .select_negotiation(
                &mut self.epochs,
                self.state.negotiation.as_ref().unwrap().epoch,
                policy,
                ShellV1ClientHello {
                    minimum_revision: revision,
                    maximum_revision: revision,
                    required_capabilities: caps,
                },
            )
            .map(|(welcome, _)| welcome)
    }

    #[cfg(feature = "experimental-native-frame-transaction")]
    fn opt_in(&mut self) {
        self.state
            .begin_frame_file_negotiation(&self.epochs, 1, Duration::from_secs(2), GRANTED)
            .unwrap();
    }
}

impl Drop for Owner {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

#[test]
fn ordinary_owner_refuses_sft_even_in_an_sft_build() {
    let mut owner = Owner::new(ContentStoreProfile::Legacy);
    owner
        .state
        .begin_file_negotiation(&owner.epochs, 1, Duration::from_secs(2), GRANTED)
        .unwrap();
    assert_eq!(
        owner.select(13, CAPS, GRANTED),
        Err(ShellTransportError::ContentAdmissionRefused(
            ContentAdmissionRefused {
                reason: 4,
                denied_capabilities: SHELL_FRAME_CAPABILITY
            }
        ))
    );
    assert!(owner.state.content_grant.is_none());
    assert_eq!(
        owner
            .select(10, CAPS & !SHELL_FRAME_CAPABILITY, GRANTED)
            .unwrap()
            .selected_revision,
        10
    );
}

#[cfg(feature = "experimental-native-frame-transaction")]
#[test]
fn owner_permission_requires_peer_opt_in_and_does_not_survive_the_handshake() {
    let mut owner = Owner::new(ContentStoreProfile::Legacy);
    owner.opt_in();
    let welcome = owner.select(13, CAPS, GRANTED).unwrap();
    assert_eq!(welcome.selected_revision, 13);
    assert_eq!(welcome.capabilities & CAPS, CAPS);
    let ordinary = owner
        .select(10, CAPS & !SHELL_FRAME_CAPABILITY, GRANTED)
        .unwrap();
    assert_eq!(ordinary.selected_revision, 10);
    assert_eq!(ordinary.capabilities & SHELL_FRAME_CAPABILITY, 0);
    owner.state.disconnect(&mut owner.epochs).unwrap();
    let grant = ContentGrant {
        connection_epoch: 2,
        content_grant_epoch: 2,
    };
    owner
        .state
        .reserve_content(&mut owner.epochs, ContentLimits::prototype(grant))
        .unwrap();
    let (mut discovery, limits, formats, bounds) =
        crate::shell_transport::owned_copy::wire_tests::config();
    discovery.connection_epoch = 2;
    owner
        .state
        .configure_image_channel(discovery, limits, formats, bounds)
        .unwrap();
    owner
        .state
        .begin_file_negotiation(&owner.epochs, 2, Duration::from_secs(2), GRANTED)
        .unwrap();
    assert_eq!(
        owner.select(13, CAPS, GRANTED),
        Err(ShellTransportError::ContentAdmissionRefused(
            ContentAdmissionRefused {
                reason: 4,
                denied_capabilities: SHELL_FRAME_CAPABILITY
            }
        ))
    );
}

#[cfg(feature = "experimental-native-frame-transaction")]
#[test]
fn sft_keeps_exact_revision_masks_authentication_and_content_policy() {
    let mut owner = Owner::new(ContentStoreProfile::Legacy);
    owner.opt_in();
    for revision in [0, 8, 9, 10, 11, 12, 14, u16::MAX] {
        assert_eq!(
            owner.select(revision, CAPS, GRANTED),
            Err(ShellTransportError::UnsupportedRevision)
        );
    }
    for bit in [11, 12, 13, 14, 16, 17, 19, 63] {
        assert_eq!(
            owner.select(13, CAPS | (1 << bit), GRANTED),
            Err(ShellTransportError::ContentAdmissionRefused(
                ContentAdmissionRefused {
                    reason: 4,
                    denied_capabilities: 1 << bit
                }
            ))
        );
    }
    for bit in [0, 7, 15] {
        assert_eq!(
            owner.select(13, CAPS & !(1 << bit), GRANTED),
            Err(ShellTransportError::MissingCapability)
        );
    }
    for (policy, reason) in [
        (ShellContentAdmissionPolicy::Denied, 1),
        (ShellContentAdmissionPolicy::Unavailable, 4),
    ] {
        assert_eq!(
            owner.select(13, CAPS, policy),
            Err(ShellTransportError::ContentAdmissionRefused(
                ContentAdmissionRefused {
                    reason,
                    denied_capabilities: 1 << 7
                }
            ))
        );
    }
    assert!(
        owner.select(13, CAPS | (1 << 8), GRANTED).is_err(),
        "SFT cannot grant denied discrete input"
    );
    owner.state.authentication_required = false;
    assert_eq!(
        owner.select(13, CAPS, GRANTED),
        Err(ShellTransportError::MissingCapability)
    );
    owner.state.authentication_required = true;
    owner
        .state
        .image_config
        .as_mut()
        .unwrap()
        .discovery
        .connection_epoch = 2;
    assert_eq!(
        owner.select(13, CAPS, GRANTED),
        Err(ShellTransportError::MissingCapability)
    );
    assert!(owner.state.content_grant.is_none());
}

#[cfg(feature = "experimental-native-frame-transaction")]
#[test]
fn sft_owner_cannot_borrow_other_profiles_or_replace_a_live_handshake() {
    for profile in [
        ContentStoreProfile::NativeLauncher,
        ContentStoreProfile::PersistentCatalog,
    ] {
        let mut owner = Owner::new(profile);
        assert_eq!(
            owner.state.begin_frame_file_negotiation(
                &owner.epochs,
                1,
                Duration::from_secs(2),
                GRANTED
            ),
            Err(ShellTransportError::WrongContentGrant)
        );
        assert!(owner.state.negotiation.is_none());
    }
    let mut owner = Owner::new(ContentStoreProfile::Legacy);
    owner
        .state
        .begin_descriptor_file_negotiation(&owner.epochs, 1, Duration::from_secs(2), GRANTED)
        .unwrap();
    assert_eq!(
        owner
            .state
            .begin_frame_file_negotiation(&owner.epochs, 1, Duration::from_secs(2), GRANTED),
        Err(ShellTransportError::WrongContentGrant)
    );
    assert!(owner.select(13, CAPS, GRANTED).is_err());
}

#[cfg(feature = "experimental-native-frame-transaction")]
#[test]
fn terminal_only_requires_a_separate_owner_permission_and_exact_peer_request() {
    use sophia_protocol::shell_files::SHELL_FRAME_TERMINAL_ONLY_CAPABILITY as OPTION;
    let mut owner = Owner::new(ContentStoreProfile::Legacy);
    owner.opt_in();
    assert!(owner.select(13, CAPS | OPTION, GRANTED).is_err());
    let mut owner = Owner::new(ContentStoreProfile::Legacy);
    owner
        .state
        .begin_terminal_frame_file_negotiation(&owner.epochs, 1, Duration::from_secs(2), GRANTED)
        .unwrap();
    assert_eq!(
        owner.select(13, CAPS, GRANTED).unwrap().capabilities & OPTION,
        0
    );
    assert_eq!(
        owner
            .select(13, CAPS | OPTION, GRANTED)
            .unwrap()
            .capabilities
            & OPTION,
        OPTION
    );
    assert!(
        owner
            .select(13, (CAPS | OPTION) & !SHELL_FRAME_CAPABILITY, GRANTED)
            .is_err()
    );
    assert!(owner.select(10, CAPS | OPTION, GRANTED).is_err());
    owner.state.negotiation = None;
    owner.opt_in();
    assert!(
        owner.select(13, CAPS | OPTION, GRANTED).is_err(),
        "permission must not survive a replacement handshake"
    );
}

#[cfg(feature = "experimental-native-frame-transaction")]
#[test]
fn lean_requires_terminal_only_and_a_separate_owner_permission() {
    use sophia_protocol::shell_files::{
        SHELL_FRAME_LEAN_CAPABILITY as LEAN, SHELL_FRAME_TERMINAL_ONLY_CAPABILITY as TERMINAL,
    };
    let mut owner = Owner::new(ContentStoreProfile::Legacy);
    owner
        .state
        .begin_terminal_frame_file_negotiation(&owner.epochs, 1, Duration::from_secs(2), GRANTED)
        .unwrap();
    assert!(
        owner.select(13, CAPS | TERMINAL | LEAN, GRANTED).is_err(),
        "terminal-only permission does not include lean"
    );
    let mut owner = Owner::new(ContentStoreProfile::Legacy);
    owner
        .state
        .begin_lean_frame_file_negotiation(&owner.epochs, 1, Duration::from_secs(2), GRANTED)
        .unwrap();
    assert!(
        owner.select(13, CAPS | LEAN, GRANTED).is_err(),
        "lean needs terminal-only"
    );
    assert_eq!(
        owner
            .select(13, CAPS | TERMINAL, GRANTED)
            .unwrap()
            .capabilities
            & LEAN,
        0
    );
    assert_eq!(
        owner
            .select(13, CAPS | TERMINAL | LEAN, GRANTED)
            .unwrap()
            .capabilities
            & (TERMINAL | LEAN),
        TERMINAL | LEAN
    );
}

//! What Session must be told before it can stand a private input service up.

use sophia_protocol::NamespaceId;
use std::num::NonZeroUsize;
use std::path::PathBuf;

/// The instance-bound cookie a client must present to be granted authority.
///
/// BOUND TO AN INSTANCE, NOT JUST SECRET. A cookie alone would let evidence
/// minted for one service be replayed at another; the instance is carried with
/// it so evidence for a different instance is refused rather than accepted as
/// merely valid. The bytes are never printed: `Debug` says a cookie is present
/// and says nothing about its value.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct PrivateInputInstanceCookie {
    pub instance: sophia_input_authority::InstanceId,
    pub cookie: [u8; 32],
}

impl core::fmt::Debug for PrivateInputInstanceCookie {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("PrivateInputInstanceCookie")
            .field("instance", &self.instance)
            .finish_non_exhaustive()
    }
}

/// Whether this service may ever issue input authority.
///
/// EXPLICIT, AND SEPARATE FROM ADMISSION. A service that admits clients is not
/// thereby a service that grants them authority to synthesise input. Enabling
/// grants is a decision an operator makes on purpose, so it is a value that
/// has to be written rather than a default that has to be remembered.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PrivateInputGrantPolicy {
    /// Admit ordinary recipients; issue nothing. The default, because a
    /// service that was accidentally left able to grant is worse than one that
    /// was accidentally left unable to.
    #[default]
    Disabled,
    /// Issue only to a connection whose setup carried verified evidence bound
    /// to this exact instance.
    EnabledWithVerifiedEvidence,
}

/// Everything Session needs to stand one private input service up.
pub struct PrivateInputConfig {
    /// Where the listener binds. The handle reports this back, so a client
    /// never reconstructs it from parts and never disagrees with the service
    /// about which socket it means.
    pub socket_path: PathBuf,
    /// The namespace this service serves, stated rather than allocated.
    ///
    /// Installed into the registry at start, with the allocator seeded past it
    /// so nothing later reuses it.
    pub namespace: NamespaceId,
    /// What that namespace may request and publish.
    ///
    /// EXPLICIT, AND NO AMBIENT GRANT. A default would decide a security
    /// question by omission; the caller states exactly what this namespace is
    /// allowed to do.
    pub profile: sophia_protocol::NamespaceProfile,
    pub capabilities: sophia_protocol::NamespaceCapabilities,
    /// The session generation this service's namespace registry runs under.
    ///
    /// Stated rather than derived from the authority instance. The registry's
    /// generation is what decides whether an admission is the current one for a
    /// client, and taking it from whatever an instance happened to report would
    /// make that decision a side effect of construction order.
    pub session_generation: u64,
    /// The seat this service's authority is bound to, with its instance.
    pub binding: sophia_input_authority::SeatBinding,
    /// The evidence a client must present to be granted authority. Required
    /// even when grants are disabled, because the setup still verifies what a
    /// client offers and refuses a wrong or malformed cookie there.
    pub cookie: PrivateInputInstanceCookie,
    pub grants: PrivateInputGrantPolicy,
    pub max_concurrent_clients: NonZeroUsize,
    pub input_capacity: NonZeroUsize,
    /// The advertised pointer button domain for this authority.
    pub advertised_buttons: u16,
    /// The output topology this headless service presents, stated rather than
    /// discovered. A service with no declared topology has no coordinate space
    /// to route into, and inventing one would make every routed position a
    /// fiction.
    pub output_topology: sophia_protocol::OutputTopologySnapshot,
    /// The frame clock the headless assembly runs on.
    ///
    /// STATED, NOT DEFAULTED. The assembly's convenience constructor picks a
    /// clock for you, which would make the timing of committed frames an
    /// accident of which constructor was called. The caller says which clock
    /// this service runs on.
    pub frame_clock: sophia_engine::DeterministicFrameClock,
}

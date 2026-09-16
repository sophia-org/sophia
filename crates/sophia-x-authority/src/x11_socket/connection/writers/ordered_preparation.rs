// Assembling a serving owner before any custody is taken.
//
// Split by subject from the owner that serves. That file answers what a
// connection's writer does; this answers how one comes to exist without the
// connection risking anything -- which is a different rule, and the whole
// reason this is separate: everything that can fail or allocate happens while
// the transport is still where it was.

/// A serving owner's parts, with its home already allocated.
///
/// Holds no transport, so a preparation that is never committed costs the
/// connection nothing.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Nothing promotes in production yet.
struct PreparedOrderedServing {
    served: XAuthorityServedConnection,
    retention: usize,
    unanswered: Vec<X11OrderedInFlight>,
    foreign: Vec<X11OrderedRefusedDelivery>,
    home: Box<Option<X11OrderedServingOwner>>,
}

#[cfg(unix)]
impl PreparedOrderedServing {
    /// Put the transport in and take the owner out.
    ///
    /// INFALLIBLE, AND IT ALLOCATES NOTHING. This is the whole of what runs
    /// between a connection's transport leaving its storage and its owner
    /// arriving there, which is why everything that could fail happened
    /// before it.
    #[cfg_attr(not(test), allow(dead_code))] // Nothing promotes in production yet.
    fn commit(self, transport: XAuthorityOrderedTransport) -> PrivateServingHome {
        let mut home = self.home;
        *home = Some(X11OrderedServingOwner {
            served: self.served,
            // Cloned before the receiver is taken: taking it leaves the minted
            // wrapper behind, and this notice with it.
            wake: transport.ordered.wake.clone(),
            queue: transport.ordered.into_receiver(),
            output: transport.output,
            shutdown: transport.shutdown,
            wire: transport.wire,
            control_pending: transport.control_pending,
            stop: transport.stop,
            in_flight: None,
            refused: None,
            ending_established: false,
            unterminated: false,
            unterminated_cause: None,
            closing: None,
            retention: self.retention,
            unanswered: self.unanswered,
            foreign: self.foreign,
        });
        PrivateServingHome(home)
    }
}

/// Where a serving owner lives once it exists.
///
/// Occupied from the moment it is made, which is only ever by `commit`.
#[cfg(unix)]
struct PrivateServingHome(Box<Option<X11OrderedServingOwner>>);

#[cfg(unix)]
impl std::ops::Deref for PrivateServingHome {
    type Target = X11OrderedServingOwner;

    fn deref(&self) -> &Self::Target {
        (*self.0)
            .as_ref()
            .expect("a serving home is filled when it is made")
    }
}

#[cfg(unix)]
impl std::ops::DerefMut for PrivateServingHome {
    fn deref_mut(&mut self) -> &mut Self::Target {
        (*self.0)
            .as_mut()
            .expect("a serving home is filled when it is made")
    }
}

#[cfg(unix)]
impl X11OrderedServingOwner {
    /// Everything a serving owner needs, assembled while its transport is
    /// still where it was.
    ///
    /// NOTHING HERE OWNS A TRANSPORT. Preparation borrows one to read what it
    /// needs -- provenance, endpoint, the retention its queue implies -- and
    /// does every fallible and allocating thing against that borrow. A refusal
    /// therefore consumes nothing: the transport is still in the storage it
    /// was read from, and the connection is exactly as it was.
    ///
    /// THE BOX IS MADE HERE TOO. Between taking a connection's transport out
    /// of storage and putting its owner back there must be nothing that can
    /// fail and nothing that allocates, and a box is an allocation; so the
    /// home is made now, empty, and committing only writes into it.
    fn prepare_for_registration(
        frontend: &crate::x11_socket::PrivateXServerFrontend,
        registration: &XServerFrontendClientRouteRegistration,
        transport: &XAuthorityOrderedTransport,
    ) -> Result<PreparedOrderedServing, X11OrderedServingRefusal> {
        // Asked again here, because a transport bound for one registration
        // must not prepare a writer for another even though both are opaque.
        if !transport.ordered.minted_by(registration) {
            return Err(X11OrderedServingRefusal::ForeignReceiver);
        }
        let endpoint = match frontend.endpoint_for(registration) {
            Ok(endpoint) => endpoint,
            Err(refusal) => return Err(X11OrderedServingRefusal::Unadmitted(refusal)),
        };
        let retention = transport.ordered.capacity();
        let mut unanswered = Vec::new();
        let mut foreign = Vec::new();
        if unanswered.try_reserve_exact(retention).is_err()
            || foreign.try_reserve_exact(retention).is_err()
        {
            return Err(X11OrderedServingRefusal::RetentionUnavailable);
        }
        Ok(PreparedOrderedServing {
            served: XAuthorityServedConnection::retained(endpoint),
            retention,
            unanswered,
            foreign,
            home: Box::new(None),
        })
    }
}

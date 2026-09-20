// A full per-client control channel is backpressure, not failure. On the
// private path a control the channel will not take is kept here, with the
// credit it holds, and sent on a later turn in the order it was routed; the
// invocation is never ended for it. The public broker's queue keeps its
// meaning: full is answered as the fault it always was, decided there.
//
// What is deferred is the message, never the operation. Routing a control
// does work before its send -- the execution is entered, the completion
// claimed, the source retained, and for a focus change the FocusOut to the
// previous client already went out -- so a retry of the whole operation
// would repeat effects that already happened. Keeping the exact message
// keeps those effects at one each, and keeps the per-client order: what a
// client is owed goes out in the order it was owed, and nothing routed
// later to that client overtakes it.

// The backlog is bounded without a bound of its own: every routed control
// holds an accepted-item credit from the settlement store, and a focus
// change adds at most one FocusOut beside its own control, so what can wait
// here is at most twice what the store accepts.
//
// LOCK ORDER: the backlog, then the client table. `flush_control_backlog`
// and `route_control_to_client` hold the backlog while reading senders;
// nothing takes the backlog while holding the client table.

/// One control a full channel would not take, with the connection it was
/// routed to: a successor registered under the same number never receives a
/// predecessor's control.
#[cfg(unix)]
struct XDeferredRoutedControl {
    incarnation: Arc<std::sync::OnceLock<PrivateAppliedClientState>>,
    control: X11RoutedControl,
}

#[cfg(unix)]
impl XServerFrontendRouteRegistry {
    /// Send one control to its client, or keep it for a later turn if the
    /// client's channel is full.
    ///
    /// The private path only: a private instance installs a control
    /// completion registry before exposure and the public broker never does,
    /// and the public broker's arm for a full queue is not this one's to
    /// change. What was kept earlier for this client goes first, so the
    /// channel sees controls in the order they were routed.
    fn route_control_to_client(
        &self,
        client: XServerFrontendClientId,
        incarnation: &Arc<std::sync::OnceLock<PrivateAppliedClientState>>,
        sender: SyncSender<X11RoutedControl>,
        control: X11RoutedControl,
    ) -> Result<(), XServerFrontendRouteError> {
        if self.control_completion.get().is_none() {
            return self.route_to_client(client, incarnation, sender, control);
        }
        let mut backlog = self
            .control_backlog
            .lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?;
        let queue = backlog.entry(client).or_default();
        let earlier = Self::send_deferred(queue, incarnation, &sender);
        if matches!(earlier, DeferredSend::Disconnected) {
            drop(backlog);
            self.remove_row_of(client, incarnation)?;
            return Err(XServerFrontendRouteError::ClientQueueDisconnected { client });
        }
        if !queue.is_empty() {
            // Something routed earlier is still waiting; this goes behind it.
            queue.push_back(XDeferredRoutedControl {
                incarnation: Arc::clone(incarnation),
                control,
            });
            return Ok(());
        }
        match sender.try_send(control) {
            Ok(()) => {
                backlog.remove(&client);
                Ok(())
            }
            Err(TrySendError::Full(control)) => {
                queue.push_back(XDeferredRoutedControl {
                    incarnation: Arc::clone(incarnation),
                    control,
                });
                Ok(())
            }
            Err(TrySendError::Disconnected(_)) => {
                backlog.remove(&client);
                drop(backlog);
                self.remove_row_of(client, incarnation)?;
                Err(XServerFrontendRouteError::ClientQueueDisconnected { client })
            }
        }
    }

    /// Send what full channels held back, for every client, as far as each
    /// channel now allows. How many controls went out.
    ///
    /// A client no longer registered, or whose channel has gone, gets
    /// nothing: what it was owed is acknowledged as `ClientGone` where an
    /// acknowledgement is owed, and dropped. A control routed to a
    /// connection that a successor has replaced under the same number is
    /// dropped the same way, never handed to the successor.
    fn flush_control_backlog(&self) -> Result<usize, XServerFrontendRouteError> {
        let mut backlog = self
            .control_backlog
            .lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?;
        let mut sent = 0;
        let mut gone = Vec::new();
        let mut disconnected = Vec::new();
        for (client, queue) in backlog.iter_mut() {
            let senders = match self.client_senders(*client) {
                Ok(senders) => senders,
                Err(XServerFrontendRouteError::UnknownClient { .. }) => {
                    gone.extend(queue.drain(..).map(|deferred| (*client, deferred)));
                    continue;
                }
                Err(error) => return Err(error),
            };
            match Self::send_deferred(queue, &senders.connection_state, &senders.control) {
                DeferredSend::Progress(count) => sent += count,
                DeferredSend::Disconnected => {
                    gone.extend(queue.drain(..).map(|deferred| (*client, deferred)));
                    disconnected.push((*client, senders.connection_state));
                }
            }
        }
        backlog.retain(|_, queue| !queue.is_empty());
        drop(backlog);
        // A channel whose receiver is gone is a connection that is gone, and
        // its row goes the way a failed send takes it: by its own identity,
        // so a successor already published under the number is left alone.
        for (client, incarnation) in disconnected {
            self.remove_row_of(client, &incarnation)?;
        }
        for (client, deferred) in gone {
            self.discard_deferred_control(client, deferred)?;
        }
        Ok(sent)
    }

    /// Send the front of one client's queue while its channel takes it.
    /// Entries routed to another incarnation of this number are dropped
    /// unsent, and count as nothing.
    fn send_deferred(
        queue: &mut VecDeque<XDeferredRoutedControl>,
        incarnation: &Arc<std::sync::OnceLock<PrivateAppliedClientState>>,
        sender: &SyncSender<X11RoutedControl>,
    ) -> DeferredSend {
        let mut sent = 0;
        while let Some(deferred) = queue.pop_front() {
            if !Arc::ptr_eq(&deferred.incarnation, incarnation) {
                continue;
            }
            match sender.try_send(deferred.control) {
                Ok(()) => sent += 1,
                Err(TrySendError::Full(control)) => {
                    queue.push_front(XDeferredRoutedControl {
                        incarnation: deferred.incarnation,
                        control,
                    });
                    break;
                }
                Err(TrySendError::Disconnected(control)) => {
                    queue.push_front(XDeferredRoutedControl {
                        incarnation: deferred.incarnation,
                        control,
                    });
                    return DeferredSend::Disconnected;
                }
            }
        }
        DeferredSend::Progress(sent)
    }

    /// A deferred control whose client is gone: acknowledged as the public
    /// router acknowledges a control to a departed client, where an
    /// acknowledgement is owed, and then dropped. A FocusOut owes none; the
    /// dependent it carries ends when it is dropped.
    fn discard_deferred_control(
        &self,
        client: XServerFrontendClientId,
        deferred: XDeferredRoutedControl,
    ) -> Result<(), XServerFrontendRouteError> {
        if let X11RoutedControl::Authority { command, .. } = &deferred.control {
            self.acknowledge_stale_control(XAuthorityClientControlCommand {
                client,
                command: *command,
            })?;
        }
        Ok(())
    }

    /// How many controls are waiting for one client's channel.
    #[cfg_attr(not(test), allow(dead_code))] // Read by controls.
    fn deferred_controls_for(&self, client: XServerFrontendClientId) -> usize {
        self.control_backlog
            .lock()
            .map(|backlog| backlog.get(&client).map_or(0, VecDeque::len))
            .unwrap_or(0)
    }
}

/// What sending a client's deferred controls did.
#[cfg(unix)]
enum DeferredSend {
    /// This many went out; the channel is full again, or the queue is empty.
    Progress(usize),
    /// The channel's receiver is gone.
    Disconnected,
}

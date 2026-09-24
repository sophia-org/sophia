// The namespace-wide broadcast and the window parent map the lifecycle
// routing reads. Included into `registry.rs`; split out to keep that file
// within the layout ledger's bound (t026).

impl XServerFrontendRouteRegistry {
    /// An event every client of a namespace is told, the requester included
    /// through its own outputs: MappingNotify, which the protocol does not
    /// let a client unselect. The clients are snapshotted and the lock
    /// released before any is written to.
    pub(crate) fn broadcast_protocol_event(
        &self,
        namespace: NamespaceId,
        except: XServerFrontendClientId,
        event: XClientEvent,
    ) -> Result<(), XServerFrontendRouteError> {
        let recipients = self
            .clients
            .lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?
            .iter()
            .filter(|(client, senders)| {
                **client != except
                    && (senders.namespace == Some(namespace)
                        || senders
                            .connection_state
                            .get()
                            .is_some_and(|state| state.namespace == namespace))
            })
            .map(|(client, _)| *client)
            .collect::<Vec<_>>();
        for recipient in recipients {
            // A peer that has gone, or cannot take it, is not a failed
            // broadcast (t090).
            self.route_protocol_contained(recipient, event)?;
        }
        Ok(())
    }

    fn register_window_parent(
        &self,
        client: XServerFrontendClientId,
        window: XResourceId,
        parent: XResourceId,
    ) -> Result<(), XServerFrontendRouteError> {
        self.window_parents
            .lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?
            .insert((client, window), parent);
        Ok(())
    }

    /// A window given a new parent by a departing client's save-set: every
    /// creator's entry for it follows, since the keys name the creator and
    /// the window outlived the client that reparented it.
    fn update_window_parent(
        &self,
        window: XResourceId,
        parent: XResourceId,
    ) -> Result<(), XServerFrontendRouteError> {
        let mut parents = self
            .window_parents
            .lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?;
        for (key, value) in parents.iter_mut() {
            if key.1 == window {
                *value = parent;
            }
        }
        Ok(())
    }

    fn remove_window_parent(
        &self,
        client: XServerFrontendClientId,
        window: XResourceId,
    ) -> Result<(), XServerFrontendRouteError> {
        self.window_parents
            .lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?
            .remove(&(client, window));
        Ok(())
    }

    fn window_ancestry(
        &self,
        client: XServerFrontendClientId,
        window: XResourceId,
    ) -> Result<Vec<XResourceId>, XServerFrontendRouteError> {
        let parents = self
            .window_parents
            .lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?;
        let mut ancestry = vec![window];
        let mut candidate = window;
        for _ in 0..64 {
            let Some(parent) = parents.get(&(client, candidate)).copied() else {
                break;
            };
            if ancestry.contains(&parent) {
                break;
            }
            ancestry.push(parent);
            candidate = parent;
        }
        Ok(ancestry)
    }

    fn window_parent(
        &self,
        window: XResourceId,
    ) -> Result<Option<XResourceId>, XServerFrontendRouteError> {
        Ok(self
            .window_parents
            .lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?
            .iter()
            .find_map(|((_, candidate), parent)| (*candidate == window).then_some(*parent)))
    }

    fn select_randr_input(
        &self,
        client: XServerFrontendClientId,
        window: XResourceId,
        mask: u16,
    ) -> Result<(), XServerFrontendRouteError> {
        let mut subscriptions = self
            .randr_subscriptions
            .lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?;
        if mask == 0 {
            subscriptions.remove(&client);
        } else {
            subscriptions.insert(client, (window, mask));
        }
        Ok(())
    }

    fn broadcast_randr_update(
        &self,
        snapshot: &sophia_protocol::OutputTopologySnapshot,
    ) -> Result<usize, XServerFrontendRouteError> {
        let size = snapshot
            .root_size()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?;
        let width =
            u16::try_from(size.width).map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?;
        let height =
            u16::try_from(size.height).map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?;
        let mm_width = u16::try_from((i64::from(size.width) * 254 + 480) / 960)
            .unwrap_or(u16::MAX)
            .max(1);
        let mm_height = u16::try_from((i64::from(size.height) * 254 + 480) / 960)
            .unwrap_or(u16::MAX)
            .max(1);
        let timestamp = u32::try_from(snapshot.generation)
            .unwrap_or(u32::MAX)
            .max(1);
        let subscriptions = self
            .randr_subscriptions
            .lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?
            .clone();
        let mut delivered = 0usize;
        for (client, (window, mask)) in subscriptions {
            if mask & 1 != 0 {
                self.route_protocol(
                    client,
                    XClientEvent::RandrScreenChange {
                        sequence: 0,
                        timestamp,
                        config_timestamp: timestamp,
                        root: XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1),
                        request_window: window,
                        width,
                        height,
                        mm_width,
                        mm_height,
                    },
                )?;
                delivered = delivered.saturating_add(1);
            }
            for output in &snapshot.outputs {
                let identity = crate::dispatch::stable_randr_identity(output.output.raw());
                let crtc = 0x1000_0000 | identity;
                let output_id = 0x2000_0000 | identity;
                let mode = crate::dispatch::stable_randr_mode_id(
                    output.logical.width,
                    output.logical.height,
                    output.refresh_millihz,
                );
                if mask & (1 << 1) != 0 {
                    self.route_protocol(
                        client,
                        XClientEvent::RandrCrtcChange {
                            sequence: 0,
                            timestamp,
                            window,
                            crtc,
                            mode,
                            x: i16::try_from(output.logical.x).unwrap_or(i16::MAX),
                            y: i16::try_from(output.logical.y).unwrap_or(i16::MAX),
                            width: u16::try_from(output.logical.width).unwrap_or(u16::MAX),
                            height: u16::try_from(output.logical.height).unwrap_or(u16::MAX),
                        },
                    )?;
                    delivered = delivered.saturating_add(1);
                }
                if mask & (1 << 2) != 0 {
                    self.route_protocol(
                        client,
                        XClientEvent::RandrOutputChange {
                            sequence: 0,
                            timestamp,
                            window,
                            output: output_id,
                            crtc,
                            mode,
                        },
                    )?;
                    delivered = delivered.saturating_add(1);
                }
            }
            if mask & (1 << 6) != 0 {
                self.route_protocol(
                    client,
                    XClientEvent::RandrResourceChange {
                        sequence: 0,
                        timestamp,
                        window,
                    },
                )?;
                delivered = delivered.saturating_add(1);
            }
        }
        Ok(delivered)
    }

}

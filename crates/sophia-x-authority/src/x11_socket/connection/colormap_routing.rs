// Colormap notices belong to the namespace screen, including on its shared root XID.

#[cfg(unix)]
fn route_colormap_events(
    routing: &XServerFrontendRouteRegistry,
    namespace: NamespaceId,
    client: XServerFrontendClientId,
    output: &mut XDispatchResult,
) -> Result<(), X11SetupSocketError> {
    let mut retained = Vec::new();
    for item in core::mem::take(&mut output.outputs) {
        let crate::XClientOutput::Event(event @ XClientEvent::ColormapNotify { window, .. }) = item
        else {
            retained.push(item);
            continue;
        };
        let subscribers = routing
            .colormap_subscribers(namespace, window)
            .map_err(|error| {
                X11SetupSocketError::new(format!(
                    "failed to inspect colormap subscriptions: {error}"
                ))
            })?;
        for recipient in subscribers
            .iter()
            .copied()
            .filter(|recipient| *recipient != client)
        {
            route_x11_peer_event(routing, recipient, event, "colormap change")?;
        }
        if subscribers.contains(&client) {
            retained.push(item);
        }
    }
    output.outputs = retained;
    Ok(())
}

#[cfg(unix)]
fn route_x11_colormap_changes(
    routing: &XServerFrontendRouteRegistry,
    namespace: NamespaceId,
    changes: &[crate::XColormapChange],
) -> Result<(), X11SetupSocketError> {
    for change in changes {
        for recipient in routing
            .colormap_subscribers(namespace, change.window)
            .map_err(|error| {
                X11SetupSocketError::new(format!(
                    "failed to inspect colormap subscriptions: {error}"
                ))
            })?
        {
            routing
                .route_protocol_contained(
                    recipient,
                    crate::XClientEvent::ColormapNotify {
                        sequence: 0,
                        window: change.window,
                        colormap: change.colormap,
                        new: change.new,
                        state: u8::from(change.installed),
                    },
                )
                .map_err(|error| {
                    X11SetupSocketError::new(format!(
                        "failed to route a freed colormap change: {error}"
                    ))
                })?;
        }
    }
    Ok(())
}

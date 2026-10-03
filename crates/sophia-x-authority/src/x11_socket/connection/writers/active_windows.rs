/// The focus changes a control command noted, put on the root with the
/// tables locked for exactly that and never under the runtime guard.
#[cfg(unix)]
fn publish_active_windows(
    atoms: &Arc<Mutex<XAtomTable>>,
    properties: &Arc<Mutex<XPropertyTable>>,
    byte_order: XByteOrder,
    changes: Vec<(NamespaceId, u32)>,
) -> Result<(), X11SetupSocketError> {
    if changes.is_empty() {
        return Ok(());
    }
    let mut atoms = atoms
        .lock()
        .map_err(|_| X11SetupSocketError::new("X11 atom table lock poisoned"))?;
    let mut properties = properties
        .lock()
        .map_err(|_| X11SetupSocketError::new("X11 property table lock poisoned"))?;
    for (namespace, window) in changes {
        if let Err(error) =
            crate::publish_active_window(&mut properties, &mut atoms, namespace, byte_order, window)
        {
            tracing::warn!(
                "sophia_x11_active_window status=unpublished namespace={} error={error:?}",
                namespace.raw()
            );
        }
    }
    Ok(())
}

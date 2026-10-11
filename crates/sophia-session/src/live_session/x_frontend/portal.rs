use super::*;

pub(crate) struct LiveClipboardPolicy {
    pub registry: Arc<Mutex<NamespaceRegistry>>,
    pub pairs: Vec<(NamespaceId, NamespaceId)>,
}
impl sophia_x_authority::XClipboardPortalPolicy for LiveClipboardPolicy {
    fn namespace(&self, id: NamespaceId) -> Option<sophia_protocol::NamespaceContext> {
        self.registry.lock().ok()?.namespace(id)
    }
    fn allow(&self, request: &sophia_protocol::PortalRequest) -> bool {
        let transfer = &request.transfer;
        transfer.kind == sophia_protocol::PortalTransferKind::Clipboard
            && self
                .pairs
                .contains(&(transfer.source_namespace, transfer.target_namespace))
    }
}

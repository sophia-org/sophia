/// The actual native teardown receipt, installed in the original lifecycle
/// gate before its record retires. A slot replacement gets a different cell.
#[cfg(unix)]
#[derive(Debug)]
struct PrivateNativeOwnerCleanup {
    identity: PrivateLifecycleIdentity,
    authority: std::sync::Weak<Mutex<crate::XInputAuthorityState>>,
    removed: crate::OrderedOwnerCleanupReceipt,
}

#[cfg(unix)]
impl PrivateEndpointIdentity {
    fn native_cleanup_receipt(
        &self,
        authority: &Arc<Mutex<crate::XInputAuthorityState>>,
    ) -> Option<&crate::OrderedOwnerCleanupReceipt> {
        let receipt = self.lifecycle.as_ref()?.cleanup.get()?;
        (receipt.identity.client == self.client
            && receipt.identity.admission == self.admission
            && receipt.identity.generation == self.generation
            && receipt.identity.namespace == self.namespace
            && std::ptr::eq(receipt.authority.as_ptr(), Arc::as_ptr(authority))
            && receipt
                .removed
                .answers_for(self.namespace, self.client.raw()))
        .then_some(&receipt.removed)
    }
}

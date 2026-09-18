/// A source-owned identity for one continuing query projection. Holders can
/// read its retirement; only the native authority that reset that projection
/// can publish it. Dropping a namespace or losing a lock publishes nothing.
#[derive(Debug)]
struct OrderedQueryScope(std::sync::Arc<std::sync::atomic::AtomicBool>);

impl Default for OrderedQueryScope {
    fn default() -> Self {
        Self(std::sync::Arc::new(std::sync::atomic::AtomicBool::new(
            false,
        )))
    }
}

// Cloning native state makes a separate authority, never shared cleanup facts.
impl Clone for OrderedQueryScope {
    fn clone(&self) -> Self {
        Self::default()
    }
}

#[derive(Clone, Debug)]
pub(crate) struct OrderedQueryScopeReceipt(std::sync::Arc<std::sync::atomic::AtomicBool>);

impl OrderedQueryScopeReceipt {
    pub(crate) fn retired(&self) -> bool {
        self.0.load(std::sync::atomic::Ordering::Acquire)
    }
}

/// Exact activation effects completed by the native owner cleanup operation.
/// Neither a client number nor an observation of absence can construct this.
#[derive(Debug)]
pub(crate) struct OrderedOwnerCleanupReceipt {
    namespace: NamespaceId,
    owner: u64,
    pointer: Option<PointerActivationStamp>,
    keyboard: Option<KeyboardActivationStamp>,
    freeze: OrderedOwnerFreezeReceipt,
}

impl OrderedOwnerCleanupReceipt {
    pub(crate) fn answers_for(&self, namespace: NamespaceId, owner: u64) -> bool {
        self.namespace == namespace && self.owner == owner
    }
    pub(crate) fn removed_pointer(&self, stamp: PointerActivationStamp) -> bool {
        self.pointer == Some(stamp) && self.freeze.answers_pointer(stamp)
    }
    pub(crate) fn removed_keyboard(&self, stamp: KeyboardActivationStamp) -> bool {
        self.keyboard == Some(stamp) && self.freeze.answers_keyboard(stamp)
    }
}

impl XInputAuthorityState {
    pub(crate) fn ordered_query_scope(
        &self,
        namespace: NamespaceId,
    ) -> Option<OrderedQueryScopeReceipt> {
        self.namespaces
            .get(&namespace)
            .map(|state| OrderedQueryScopeReceipt(state.query_scope.0.clone()))
    }

    pub(crate) fn cleanup_ordered_owner(
        &mut self,
        namespace: NamespaceId,
        owner: u64,
    ) -> OrderedOwnerCleanupReceipt {
        let pointer = self.namespaces.get(&namespace).and_then(|state| {
            match (state.pointer_activation, state.pointer) {
                (PointerActivationState::Applied(stamp), Some(active)) if active.owner == owner => {
                    Some(stamp)
                }
                _ => None,
            }
        });
        let keyboard = self
            .keyboard_activation(namespace)
            .ok()
            .flatten()
            .filter(|activation| activation.recipient().owner == owner)
            .map(|activation| activation.stamp());
        let freeze = self.cleanup_ordered_freeze_owner(namespace, owner);
        self.cleanup_owner(owner);
        OrderedOwnerCleanupReceipt {
            namespace,
            owner,
            pointer,
            keyboard,
            freeze,
        }
    }
}

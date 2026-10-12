use super::*;

#[derive(Debug)]
pub(super) struct NamespaceRecord {
    pub context: NamespaceContext,
    pub active: Arc<AtomicBool>,
}
impl NamespaceRecord {
    pub fn new(context: NamespaceContext) -> Self {
        Self {
            context,
            active: Arc::new(AtomicBool::new(true)),
        }
    }
}

#[derive(Debug)]
pub(super) struct AdmissionRecord {
    pub context: ClientAdmissionContext,
    active: Arc<AtomicBool>,
}
impl AdmissionRecord {
    pub fn live(&self) -> bool {
        self.active.load(Ordering::Acquire)
    }
    pub fn revoke(&self) {
        self.active.store(false, Ordering::Release);
    }
}

/// A transport owns this handle for its entire lifetime, including wire resets.
/// Dropping it revokes admission without waiting for the registry mutex. Facts
/// stay passive in ClientAdmissionContext; this owner supplies live checks.
#[derive(Debug)]
pub struct AdmissionLease {
    context: ClientAdmissionContext,
    active: Arc<AtomicBool>,
    namespace: Arc<AtomicBool>,
    registry: Arc<AtomicBool>,
}
impl AdmissionLease {
    pub fn context(&self) -> ClientAdmissionContext {
        self.context
    }
    pub fn is_live(&self) -> bool {
        self.registry.load(Ordering::Acquire)
            && self.namespace.load(Ordering::Acquire)
            && self.active.load(Ordering::Acquire)
    }
}
impl Drop for AdmissionLease {
    fn drop(&mut self) {
        self.active.store(false, Ordering::Release);
    }
}

/// An unpublished admission. Dropping it burns its ID but changes no authority.
pub struct PreparedAdmission<'a> {
    registry: &'a mut NamespaceRegistry,
    context: ClientAdmissionContext,
}
impl<'a> PreparedAdmission<'a> {
    pub(super) fn new(
        registry: &'a mut NamespaceRegistry,
        context: ClientAdmissionContext,
    ) -> Self {
        Self { registry, context }
    }
    pub fn context(&self) -> ClientAdmissionContext {
        self.context
    }
    /// Publish an admission whose lifetime is managed directly by the registry.
    pub fn publish(mut self) -> ClientAdmissionContext {
        self.insert();
        self.context
    }
    /// Publish an admission whose lifetime also ends with its transport owner.
    pub fn publish_lease(mut self) -> AdmissionLease {
        let active = self.insert();
        AdmissionLease {
            context: self.context,
            active,
            namespace: self.registry.namespaces[&self.context.namespace.id]
                .active
                .clone(),
            registry: self.registry.live.clone(),
        }
    }
    fn insert(&mut self) -> Arc<AtomicBool> {
        let active = Arc::new(AtomicBool::new(true));
        self.registry.admissions.insert(
            self.context.client_id,
            AdmissionRecord {
                context: self.context,
                active: active.clone(),
            },
        );
        active
    }
}

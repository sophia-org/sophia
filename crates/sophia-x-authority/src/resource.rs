use std::collections::BTreeMap;

use sophia_protocol::{AuthorityLocalId, NamespaceId};

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct XResourceId {
    pub local: AuthorityLocalId,
}

impl XResourceId {
    pub const NONE: Self = Self {
        local: AuthorityLocalId::NONE,
    };

    pub const fn new(raw: u64, generation: u32) -> Self {
        Self {
            local: AuthorityLocalId::new(raw, generation),
        }
    }

    pub const fn is_valid(self) -> bool {
        self.local.is_valid()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum XResourceKind {
    Window,
    Pixmap,
    Atom,
    Property,
    GraphicsContext,
    Font,
    Cursor,
    Colormap,
    Fence,
    Region,
    SyncCounter,
    Picture,
    GlyphSet,
}

/// What a window is painted with when it becomes viewable.
///
/// X11 makes this four-valued and the difference matters: an undefined
/// background is not black, it is "do not paint", and whatever was already on
/// the screen stays. A window whose background is undefined but painted
/// anyway erases what it was placed over.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum XWindowBackground {
    /// `BackPixmap` None: nothing is painted.
    Undefined,
    /// `BackPixmap` ParentRelative: whatever the parent's background is.
    ParentRelative,
    Pixel(u32),
    Pixmap(crate::XResourceId),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum XMapState {
    Unmapped,
    Unviewable,
    Viewable,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct XResourceRecord {
    pub id: XResourceId,
    pub kind: XResourceKind,
    pub owner_namespace: NamespaceId,
    pub generation: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum XAuthorityAccessError {
    InvalidResource,
    InvalidNamespace,
    InvalidSurface,
    UnknownResource,
    WrongResourceKind,
    CrossNamespaceDenied,
    StaleGeneration,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct XResourceTable {
    records: BTreeMap<XResourceId, XResourceRecord>,
}

impl XResourceTable {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(
        &mut self,
        id: XResourceId,
        kind: XResourceKind,
        owner_namespace: NamespaceId,
        generation: u64,
    ) -> Result<(), XAuthorityAccessError> {
        if !id.is_valid() {
            return Err(XAuthorityAccessError::InvalidResource);
        }
        if !owner_namespace.is_valid() {
            return Err(XAuthorityAccessError::InvalidNamespace);
        }
        if self.records.contains_key(&id) {
            return Err(XAuthorityAccessError::InvalidResource);
        }

        self.records.insert(
            id,
            XResourceRecord {
                id,
                kind,
                owner_namespace,
                generation,
            },
        );
        Ok(())
    }

    pub fn get(&self, id: XResourceId) -> Option<&XResourceRecord> {
        self.records.get(&id)
    }

    pub fn lookup(
        &self,
        requester_namespace: NamespaceId,
        id: XResourceId,
        expected_kind: XResourceKind,
    ) -> Result<&XResourceRecord, XAuthorityAccessError> {
        if !requester_namespace.is_valid() {
            return Err(XAuthorityAccessError::InvalidNamespace);
        }

        let record = self
            .records
            .get(&id)
            .ok_or(XAuthorityAccessError::UnknownResource)?;

        if record.kind != expected_kind {
            return Err(XAuthorityAccessError::WrongResourceKind);
        }
        if record.owner_namespace != requester_namespace {
            return Err(XAuthorityAccessError::CrossNamespaceDenied);
        }

        Ok(record)
    }

    pub fn remove(&mut self, id: XResourceId) -> Option<XResourceRecord> {
        self.records.remove(&id)
    }

    pub fn records_for_namespace_in_client_range(
        &self,
        namespace: NamespaceId,
        range: crate::XWireClientResourceRange,
    ) -> Vec<XResourceRecord> {
        self.records
            .values()
            .filter(|record| {
                record.owner_namespace == namespace
                    && u32::try_from(record.id.local.raw())
                        .is_ok_and(|raw| range.owns_new_resource(raw))
            })
            .cloned()
            .collect()
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }
}

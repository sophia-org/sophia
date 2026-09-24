use std::collections::{BTreeMap, BTreeSet};

use sophia_protocol::{
    AxisSpan, LayoutNodeKind, NamespaceId, OutputEdge, OutputReservation, PolicyPresentationState,
    Rect, Size, SurfaceConstraints, SurfacePlacementPreference,
};

use crate::{
    X_ATOM_ATOM, X_ATOM_CARDINAL, X_ATOM_NAME_NET_ACTIVE_WINDOW, X_ATOM_NAME_NET_SUPPORTED,
    X_ATOM_NAME_NET_SUPPORTING_WM_CHECK, X_ATOM_NAME_NET_WM_NAME, X_ATOM_NAME_NET_WM_STATE,
    X_ATOM_NAME_NET_WM_STATE_FULLSCREEN, X_ATOM_NAME_NET_WM_STATE_HIDDEN,
    X_ATOM_NAME_NET_WM_STATE_MAXIMIZED_HORZ, X_ATOM_NAME_NET_WM_STATE_MAXIMIZED_VERT,
    X_ATOM_NAME_NET_WM_STRUT, X_ATOM_NAME_NET_WM_STRUT_PARTIAL, X_ATOM_NAME_NET_WM_WINDOW_TYPE,
    X_ATOM_NAME_UTF8_STRING, X_ATOM_NAME_WM_STATE, X_ATOM_WINDOW, XAtom, XAtomError, XAtomTable,
    XByteOrder, XResourceId, is_metadata_candidate_name,
};

pub const X_PROPERTY_MAX_VALUE_BYTES: usize = 256 * 1024;
pub const X_PROPERTY_MAX_TABLE_BYTES: usize = 4 * 1024 * 1024;
pub const X_PROPERTY_ANY_TYPE: XAtom = 0;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum XPropertyMode {
    Replace,
    Prepend,
    Append,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct XPropertyChange {
    pub mode: XPropertyMode,
    pub window: XResourceId,
    pub property: XAtom,
    pub property_type: XAtom,
    pub format: u8,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct XPropertyRead {
    pub delete: bool,
    pub window: XResourceId,
    pub property: XAtom,
    pub property_type: XAtom,
    pub long_offset: u32,
    pub long_length: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct XPropertyReadReply {
    pub property_type: XAtom,
    pub format: u8,
    pub bytes_after: u32,
    pub item_count: u32,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct XPropertyReadOutcome {
    pub reply: XPropertyReadReply,
    pub deleted: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct XPropertyRecord {
    pub namespace: NamespaceId,
    pub window: XResourceId,
    pub property: XAtom,
    pub property_type: XAtom,
    pub format: u8,
    pub bytes: Vec<u8>,
    pub generation: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct XMetadataPropertyCandidate {
    pub namespace: NamespaceId,
    pub window: XResourceId,
    pub property: XAtom,
    pub property_name: String,
    pub property_type: XAtom,
    pub property_type_name: Option<String>,
    pub format: u8,
    pub byte_len: usize,
    pub generation: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum XPropertyError {
    InvalidNamespace,
    InvalidWindow,
    InvalidFormat(u8),
    ValueTooLarge { len: usize, max: usize },
    TableTooLarge { len: usize, max: usize },
    TypeMismatch,
    InvalidOffset,
    AuthorityOwned,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum XPresentationPropertyError {
    InvalidState,
    Atom(XAtomError),
    Property(XPropertyError),
}

impl From<XAtomError> for XPresentationPropertyError {
    fn from(error: XAtomError) -> Self {
        Self::Atom(error)
    }
}

impl From<XPropertyError> for XPresentationPropertyError {
    fn from(error: XPropertyError) -> Self {
        Self::Property(error)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum XOutputReservationDecodeError {
    InvalidRoot,
    InvalidType,
    InvalidFormat,
    InvalidLength,
    ValueOutOfRange,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum XSizeHintsDecodeError {
    InvalidType,
    InvalidFormat,
    InvalidLength,
    InvalidExtent,
    InvalidBounds,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum XTransientForDecodeError {
    InvalidType,
    InvalidFormat,
    InvalidLength,
    InvalidWindow,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum XWindowTypeDecodeError {
    InvalidType,
    InvalidFormat,
    InvalidLength,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct XWindowTypeFacts {
    pub kind: LayoutNodeKind,
    pub placement_preference: SurfacePlacementPreference,
    /// Desktop and dock windows are frontend-positioned shell surfaces. All
    /// other non-override-redirect root children remain redirected to policy.
    pub client_positioned: bool,
}

impl Default for XWindowTypeFacts {
    fn default() -> Self {
        Self {
            kind: LayoutNodeKind::Toplevel,
            placement_preference: SurfacePlacementPreference::Default,
            client_positioned: false,
        }
    }
}

/// Reduces EWMH functional window types to the presentation distinction the
/// Engine needs. Unknown extension atoms are skipped, as required by EWMH;
/// a missing recognized type falls back to a normal policy-managed toplevel.
pub fn decode_x_window_type_facts(
    record: &XPropertyRecord,
    atoms: &XAtomTable,
    byte_order: XByteOrder,
) -> Option<Result<XWindowTypeFacts, XWindowTypeDecodeError>> {
    if atoms.name(record.property) != Some("_NET_WM_WINDOW_TYPE") {
        return None;
    }
    if atoms.name(record.property_type) != Some("ATOM") {
        return Some(Err(XWindowTypeDecodeError::InvalidType));
    }
    if record.format != 32 {
        return Some(Err(XWindowTypeDecodeError::InvalidFormat));
    }
    if record.bytes.is_empty() || !record.bytes.len().is_multiple_of(4) {
        return Some(Err(XWindowTypeDecodeError::InvalidLength));
    }

    let facts = record.bytes.chunks_exact(4).find_map(|bytes| {
        let atom = byte_order.u32(bytes);
        match atoms.name(atom) {
            Some("_NET_WM_WINDOW_TYPE_NORMAL") => Some(XWindowTypeFacts::default()),
            Some("_NET_WM_WINDOW_TYPE_DESKTOP") | Some("_NET_WM_WINDOW_TYPE_DOCK") => {
                Some(XWindowTypeFacts {
                    kind: LayoutNodeKind::Utility,
                    placement_preference: SurfacePlacementPreference::Default,
                    client_positioned: true,
                })
            }
            Some("_NET_WM_WINDOW_TYPE_TOOLBAR") | Some("_NET_WM_WINDOW_TYPE_UTILITY") => {
                Some(XWindowTypeFacts {
                    kind: LayoutNodeKind::Utility,
                    placement_preference: SurfacePlacementPreference::Floating,
                    client_positioned: false,
                })
            }
            Some("_NET_WM_WINDOW_TYPE_SPLASH") | Some("_NET_WM_WINDOW_TYPE_DIALOG") => {
                Some(XWindowTypeFacts {
                    kind: LayoutNodeKind::Dialog,
                    placement_preference: SurfacePlacementPreference::Floating,
                    client_positioned: false,
                })
            }
            Some("_NET_WM_WINDOW_TYPE_MENU")
            | Some("_NET_WM_WINDOW_TYPE_DROPDOWN_MENU")
            | Some("_NET_WM_WINDOW_TYPE_POPUP_MENU")
            | Some("_NET_WM_WINDOW_TYPE_TOOLTIP")
            | Some("_NET_WM_WINDOW_TYPE_NOTIFICATION")
            | Some("_NET_WM_WINDOW_TYPE_COMBO")
            | Some("_NET_WM_WINDOW_TYPE_DND") => Some(XWindowTypeFacts {
                kind: LayoutNodeKind::Popup,
                placement_preference: SurfacePlacementPreference::Floating,
                client_positioned: false,
            }),
            _ => None,
        }
    });
    Some(Ok(facts.unwrap_or_default()))
}

pub fn decode_x_transient_for(
    record: &XPropertyRecord,
    atoms: &XAtomTable,
    byte_order: XByteOrder,
) -> Option<Result<XResourceId, XTransientForDecodeError>> {
    if atoms.name(record.property) != Some("WM_TRANSIENT_FOR") {
        return None;
    }
    if atoms.name(record.property_type) != Some("WINDOW") {
        return Some(Err(XTransientForDecodeError::InvalidType));
    }
    if record.format != 32 {
        return Some(Err(XTransientForDecodeError::InvalidFormat));
    }
    if record.bytes.len() != 4 {
        return Some(Err(XTransientForDecodeError::InvalidLength));
    }
    let raw = u64::from(byte_order.u32(&record.bytes));
    let owner = XResourceId::new(raw, 1);
    if !owner.is_valid() {
        return Some(Err(XTransientForDecodeError::InvalidWindow));
    }
    Some(Ok(owner))
}

pub fn decode_x_size_hints(
    record: &XPropertyRecord,
    atoms: &XAtomTable,
    byte_order: XByteOrder,
) -> Option<Result<SurfaceConstraints, XSizeHintsDecodeError>> {
    if atoms.name(record.property) != Some("WM_NORMAL_HINTS") {
        return None;
    }
    if atoms.name(record.property_type) != Some("WM_SIZE_HINTS") {
        return Some(Err(XSizeHintsDecodeError::InvalidType));
    }
    if record.format != 32 {
        return Some(Err(XSizeHintsDecodeError::InvalidFormat));
    }
    if record.bytes.len() < 9 * 4 {
        return Some(Err(XSizeHintsDecodeError::InvalidLength));
    }
    let value = |index: usize| byte_order.u32(&record.bytes[index * 4..index * 4 + 4]) as i32;
    let flags = value(0) as u32;
    let extent = |width_index: usize, height_index: usize| {
        let size = Size {
            width: value(width_index),
            height: value(height_index),
        };
        (size.width > 0 && size.height > 0)
            .then_some(size)
            .ok_or(XSizeHintsDecodeError::InvalidExtent)
    };
    const P_MIN_SIZE: u32 = 1 << 4;
    const P_MAX_SIZE: u32 = 1 << 5;
    let min_size = if flags & P_MIN_SIZE != 0 {
        match extent(5, 6) {
            Ok(size) => Some(size),
            Err(error) => return Some(Err(error)),
        }
    } else {
        None
    };
    let max_size = if flags & P_MAX_SIZE != 0 {
        match extent(7, 8) {
            Ok(size) => Some(size),
            Err(error) => return Some(Err(error)),
        }
    } else {
        None
    };
    if matches!((min_size, max_size), (Some(minimum), Some(maximum))
        if minimum.width > maximum.width || minimum.height > maximum.height)
    {
        return Some(Err(XSizeHintsDecodeError::InvalidBounds));
    }
    Some(Ok(SurfaceConstraints { min_size, max_size }))
}

pub fn decode_x_output_reservations(
    record: &XPropertyRecord,
    atoms: &XAtomTable,
    byte_order: XByteOrder,
    root: Rect,
) -> Option<Result<Vec<OutputReservation>, XOutputReservationDecodeError>> {
    let property_name = atoms.name(record.property)?;
    match property_name {
        X_ATOM_NAME_NET_WM_STRUT_PARTIAL => {
            Some(decode_partial_output_reservations(record, byte_order, root))
        }
        X_ATOM_NAME_NET_WM_STRUT => {
            Some(decode_legacy_output_reservations(record, byte_order, root))
        }
        _ => None,
    }
}

pub fn x_output_reservations_for_window(
    properties: &XPropertyTable,
    atoms: &XAtomTable,
    namespace: NamespaceId,
    window: XResourceId,
    byte_order: XByteOrder,
    root: Rect,
) -> Vec<OutputReservation> {
    let partial = atoms
        .atom(X_ATOM_NAME_NET_WM_STRUT_PARTIAL)
        .and_then(|property| properties.get(namespace, window, property))
        .and_then(|record| decode_x_output_reservations(record, atoms, byte_order, root))
        .and_then(Result::ok);
    if let Some(reservations) = partial {
        return reservations;
    }

    atoms
        .atom(X_ATOM_NAME_NET_WM_STRUT)
        .and_then(|property| properties.get(namespace, window, property))
        .and_then(|record| decode_x_output_reservations(record, atoms, byte_order, root))
        .and_then(Result::ok)
        .unwrap_or_default()
}

pub fn metadata_property_candidate(
    record: &XPropertyRecord,
    atoms: &XAtomTable,
) -> Option<XMetadataPropertyCandidate> {
    let property_name = atoms.name(record.property)?;
    if !is_metadata_candidate_name(property_name) {
        return None;
    }
    Some(XMetadataPropertyCandidate {
        namespace: record.namespace,
        window: record.window,
        property: record.property,
        property_name: property_name.to_owned(),
        property_type: record.property_type,
        property_type_name: atoms
            .name(record.property_type)
            .map(std::borrow::ToOwned::to_owned),
        format: record.format,
        byte_len: record.bytes.len(),
        generation: record.generation,
    })
}

fn decode_partial_output_reservations(
    record: &XPropertyRecord,
    byte_order: XByteOrder,
    root: Rect,
) -> Result<Vec<OutputReservation>, XOutputReservationDecodeError> {
    const PARTIAL_CARDINAL_COUNT: usize = 12;
    let values = decode_cardinals(record, byte_order, PARTIAL_CARDINAL_COUNT)?;
    let root_horizontal = root_horizontal_span(root)?;
    let root_vertical = root_vertical_span(root)?;
    let mut reservations = Vec::with_capacity(4);
    push_output_reservation(
        &mut reservations,
        OutputEdge::Left,
        values[0],
        values[4],
        values[5],
        root.width,
        root_vertical,
    )?;
    push_output_reservation(
        &mut reservations,
        OutputEdge::Right,
        values[1],
        values[6],
        values[7],
        root.width,
        root_vertical,
    )?;
    push_output_reservation(
        &mut reservations,
        OutputEdge::Top,
        values[2],
        values[8],
        values[9],
        root.height,
        root_horizontal,
    )?;
    push_output_reservation(
        &mut reservations,
        OutputEdge::Bottom,
        values[3],
        values[10],
        values[11],
        root.height,
        root_horizontal,
    )?;
    Ok(reservations)
}

fn decode_legacy_output_reservations(
    record: &XPropertyRecord,
    byte_order: XByteOrder,
    root: Rect,
) -> Result<Vec<OutputReservation>, XOutputReservationDecodeError> {
    const LEGACY_CARDINAL_COUNT: usize = 4;
    let values = decode_cardinals(record, byte_order, LEGACY_CARDINAL_COUNT)?;
    let horizontal = root_horizontal_span(root)?;
    let vertical = root_vertical_span(root)?;
    let mut reservations = Vec::with_capacity(4);
    push_legacy_output_reservation(
        &mut reservations,
        OutputEdge::Left,
        values[0],
        root.width,
        vertical,
    )?;
    push_legacy_output_reservation(
        &mut reservations,
        OutputEdge::Right,
        values[1],
        root.width,
        vertical,
    )?;
    push_legacy_output_reservation(
        &mut reservations,
        OutputEdge::Top,
        values[2],
        root.height,
        horizontal,
    )?;
    push_legacy_output_reservation(
        &mut reservations,
        OutputEdge::Bottom,
        values[3],
        root.height,
        horizontal,
    )?;
    Ok(reservations)
}

fn decode_cardinals(
    record: &XPropertyRecord,
    byte_order: XByteOrder,
    count: usize,
) -> Result<Vec<u32>, XOutputReservationDecodeError> {
    if record.property_type != X_ATOM_CARDINAL {
        return Err(XOutputReservationDecodeError::InvalidType);
    }
    if record.format != 32 {
        return Err(XOutputReservationDecodeError::InvalidFormat);
    }
    if record.bytes.len() != count.saturating_mul(4) {
        return Err(XOutputReservationDecodeError::InvalidLength);
    }
    Ok(record
        .bytes
        .chunks_exact(4)
        .map(|bytes| byte_order.u32(bytes))
        .collect())
}

fn push_output_reservation(
    reservations: &mut Vec<OutputReservation>,
    edge: OutputEdge,
    depth: u32,
    start: u32,
    inclusive_end: u32,
    maximum_depth: i32,
    root_span: AxisSpan,
) -> Result<(), XOutputReservationDecodeError> {
    if depth == 0 {
        return Ok(());
    }
    let depth = i32::try_from(depth).map_err(|_| XOutputReservationDecodeError::ValueOutOfRange)?;
    let start = i32::try_from(start).map_err(|_| XOutputReservationDecodeError::ValueOutOfRange)?;
    let end = inclusive_end
        .checked_add(1)
        .and_then(|value| i32::try_from(value).ok())
        .ok_or(XOutputReservationDecodeError::ValueOutOfRange)?;
    let span = AxisSpan { start, end };
    if depth > maximum_depth
        || span.is_empty()
        || span.start < root_span.start
        || span.end > root_span.end
    {
        return Err(XOutputReservationDecodeError::ValueOutOfRange);
    }
    reservations.push(OutputReservation { edge, depth, span });
    Ok(())
}

fn push_legacy_output_reservation(
    reservations: &mut Vec<OutputReservation>,
    edge: OutputEdge,
    depth: u32,
    maximum_depth: i32,
    span: AxisSpan,
) -> Result<(), XOutputReservationDecodeError> {
    if depth == 0 {
        return Ok(());
    }
    let depth = i32::try_from(depth).map_err(|_| XOutputReservationDecodeError::ValueOutOfRange)?;
    if depth > maximum_depth {
        return Err(XOutputReservationDecodeError::ValueOutOfRange);
    }
    reservations.push(OutputReservation { edge, depth, span });
    Ok(())
}

fn root_horizontal_span(root: Rect) -> Result<AxisSpan, XOutputReservationDecodeError> {
    if root.is_empty() {
        return Err(XOutputReservationDecodeError::InvalidRoot);
    }
    Ok(AxisSpan {
        start: root.x,
        end: root
            .x
            .checked_add(root.width)
            .ok_or(XOutputReservationDecodeError::InvalidRoot)?,
    })
}

fn root_vertical_span(root: Rect) -> Result<AxisSpan, XOutputReservationDecodeError> {
    if root.is_empty() {
        return Err(XOutputReservationDecodeError::InvalidRoot);
    }
    Ok(AxisSpan {
        start: root.y,
        end: root
            .y
            .checked_add(root.height)
            .ok_or(XOutputReservationDecodeError::InvalidRoot)?,
    })
}

impl core::fmt::Display for XPropertyError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for XPropertyError {}

impl core::fmt::Display for XPresentationPropertyError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for XPresentationPropertyError {}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct XPropertyTable {
    records: BTreeMap<(NamespaceId, XResourceId, XAtom), XPropertyRecord>,
    engine_owned: BTreeSet<(NamespaceId, XResourceId, XAtom)>,
}

impl XPropertyTable {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn apply_change(
        &mut self,
        namespace: NamespaceId,
        change: XPropertyChange,
    ) -> Result<XPropertyRecord, XPropertyError> {
        self.apply_change_inner(namespace, change, false)
    }

    /// A withdrawn client may replace its next map's EWMH hints. This does
    /// not alter committed presentation state or release property ownership.
    pub(crate) fn apply_initial_net_wm_state(
        &mut self,
        namespace: NamespaceId,
        change: XPropertyChange,
        atoms: &XAtomTable,
    ) -> Result<XPropertyRecord, XPropertyError> {
        if atoms.name(change.property) != Some(X_ATOM_NAME_NET_WM_STATE) {
            return Err(XPropertyError::AuthorityOwned);
        }
        self.apply_change_inner(namespace, change, true)
    }

    fn apply_change_inner(
        &mut self,
        namespace: NamespaceId,
        change: XPropertyChange,
        initial_net_wm_state: bool,
    ) -> Result<XPropertyRecord, XPropertyError> {
        if !namespace.is_valid() {
            return Err(XPropertyError::InvalidNamespace);
        }
        if !change.window.is_valid() {
            return Err(XPropertyError::InvalidWindow);
        }
        validate_property_format(change.format)?;
        if change.bytes.len() > X_PROPERTY_MAX_VALUE_BYTES {
            return Err(XPropertyError::ValueTooLarge {
                len: change.bytes.len(),
                max: X_PROPERTY_MAX_VALUE_BYTES,
            });
        }

        let key = (namespace, change.window, change.property);
        if self.engine_owned.contains(&key) && !initial_net_wm_state {
            return Err(XPropertyError::AuthorityOwned);
        }
        let previous = self.records.get(&key);
        let previous_len = previous.map_or(0, |record| record.bytes.len());
        let generation = previous
            .map(|record| record.generation.saturating_add(1))
            .unwrap_or(1);
        let bytes = match (change.mode, previous) {
            (XPropertyMode::Replace, _) | (_, None) => change.bytes,
            (XPropertyMode::Append, Some(record)) => {
                ensure_same_property_shape(record, &change)?;
                joined_bytes(&record.bytes, &change.bytes)?
            }
            (XPropertyMode::Prepend, Some(record)) => {
                ensure_same_property_shape(record, &change)?;
                joined_bytes(&change.bytes, &record.bytes)?
            }
        };
        let table_len = self
            .records
            .values()
            .try_fold(0usize, |total, record| {
                total.checked_add(record.bytes.len())
            })
            .and_then(|total| total.checked_sub(previous_len))
            .and_then(|total| total.checked_add(bytes.len()))
            .ok_or(XPropertyError::TableTooLarge {
                len: usize::MAX,
                max: X_PROPERTY_MAX_TABLE_BYTES,
            })?;
        if table_len > X_PROPERTY_MAX_TABLE_BYTES {
            return Err(XPropertyError::TableTooLarge {
                len: table_len,
                max: X_PROPERTY_MAX_TABLE_BYTES,
            });
        }

        let record = XPropertyRecord {
            namespace,
            window: change.window,
            property: change.property,
            property_type: change.property_type,
            format: change.format,
            bytes,
            generation,
        };
        self.records.insert(key, record.clone());
        Ok(record)
    }

    /// RotateProperties: the values of `properties` move `delta` places
    /// along the list, in place; every named property must exist on the
    /// window and be named once, else nothing moves (BadMatch); an
    /// Engine-owned one refuses (BadAccess). Returns the properties whose
    /// value changed, each owed a PropertyNotify.
    pub fn rotate(
        &mut self,
        namespace: NamespaceId,
        window: XResourceId,
        properties: &[XAtom],
        delta: i16,
    ) -> Result<Vec<XAtom>, XPropertyError> {
        if !namespace.is_valid() {
            return Err(XPropertyError::InvalidNamespace);
        }
        if !window.is_valid() {
            return Err(XPropertyError::InvalidWindow);
        }
        let count = properties.len();
        let distinct = properties.iter().collect::<std::collections::BTreeSet<_>>();
        if distinct.len() != count {
            return Err(XPropertyError::TypeMismatch);
        }
        for property in properties {
            let key = (namespace, window, *property);
            if !self.records.contains_key(&key) {
                return Err(XPropertyError::TypeMismatch);
            }
            if self.engine_owned.contains(&key) {
                return Err(XPropertyError::AuthorityOwned);
            }
        }
        if count == 0 {
            return Ok(Vec::new());
        }
        let shift = (i64::from(delta)).rem_euclid(count as i64) as usize;
        if shift == 0 {
            return Ok(Vec::new());
        }
        let values = properties
            .iter()
            .map(|property| {
                let record = &self.records[&(namespace, window, *property)];
                (record.property_type, record.format, record.bytes.clone())
            })
            .collect::<Vec<_>>();
        // The protocol: the value of properties[i] becomes that of
        // properties[(i + delta) mod count].
        for (index, property) in properties.iter().enumerate() {
            let (property_type, format, bytes) = values[(index + shift) % count].clone();
            let record = self
                .records
                .get_mut(&(namespace, window, *property))
                .expect("checked present above");
            record.property_type = property_type;
            record.format = format;
            record.bytes = bytes;
            record.generation = record.generation.saturating_add(1);
        }
        Ok(properties.to_vec())
    }

    pub fn get(
        &self,
        namespace: NamespaceId,
        window: XResourceId,
        property: XAtom,
    ) -> Option<&XPropertyRecord> {
        self.records.get(&(namespace, window, property))
    }

    pub fn properties_for_window(&self, namespace: NamespaceId, window: XResourceId) -> Vec<XAtom> {
        self.records
            .keys()
            .filter_map(|(record_namespace, record_window, property)| {
                (*record_namespace == namespace && *record_window == window).then_some(*property)
            })
            .collect()
    }

    pub fn windows_with_property(
        &self,
        namespace: NamespaceId,
        property: XAtom,
    ) -> Vec<XResourceId> {
        self.records
            .keys()
            .filter_map(|(record_namespace, window, record_property)| {
                (*record_namespace == namespace && *record_property == property).then_some(*window)
            })
            .collect()
    }

    pub fn remove(
        &mut self,
        namespace: NamespaceId,
        window: XResourceId,
        property: XAtom,
    ) -> Result<Option<XPropertyRecord>, XPropertyError> {
        let key = (namespace, window, property);
        if self.engine_owned.contains(&key) {
            return Err(XPropertyError::AuthorityOwned);
        }
        Ok(self.records.remove(&key))
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    pub fn read_property(
        &mut self,
        namespace: NamespaceId,
        read: XPropertyRead,
    ) -> Result<XPropertyReadOutcome, XPropertyError> {
        if !namespace.is_valid() {
            return Err(XPropertyError::InvalidNamespace);
        }
        if !read.window.is_valid() {
            return Err(XPropertyError::InvalidWindow);
        }

        let Some(record) = self.get(namespace, read.window, read.property) else {
            return Ok(XPropertyReadOutcome {
                reply: XPropertyReadReply {
                    property_type: X_PROPERTY_ANY_TYPE,
                    format: 0,
                    bytes_after: 0,
                    item_count: 0,
                    bytes: Vec::new(),
                },
                deleted: false,
            });
        };

        let reply = read_property_value(
            record.property_type,
            record.format,
            &record.bytes,
            read.property_type,
            read.long_offset,
            read.long_length,
        )?;
        let deleted = read.delete
            && reply.property_type != X_PROPERTY_ANY_TYPE
            && (read.property_type == X_PROPERTY_ANY_TYPE
                || read.property_type == reply.property_type)
            && reply.bytes_after == 0
            && !self
                .engine_owned
                .contains(&(namespace, read.window, read.property));
        if deleted {
            let _ = self.remove(namespace, read.window, read.property)?;
        }
        Ok(XPropertyReadOutcome { reply, deleted })
    }
}

/// Installs Engine-authoritative logical presentation feedback as one bounded
/// property-table transition. Geometry and policy remain outside this X11
/// compatibility adapter; only standard client-visible state is materialized.
pub fn apply_engine_presentation_state(
    properties: &mut XPropertyTable,
    atoms: &mut XAtomTable,
    namespace: NamespaceId,
    window: XResourceId,
    byte_order: XByteOrder,
    state: PolicyPresentationState,
) -> Result<Vec<XAtom>, XPresentationPropertyError> {
    if !namespace.is_valid() {
        return Err(XPropertyError::InvalidNamespace.into());
    }
    if !window.is_valid() {
        return Err(XPropertyError::InvalidWindow.into());
    }
    if (state.fullscreen && state.maximized)
        || (state.minimized && (state.fullscreen || state.maximized))
    {
        return Err(XPresentationPropertyError::InvalidState);
    }

    let mut atom = |name: &str| -> Result<XAtom, XPresentationPropertyError> {
        atoms
            .intern(name, false)?
            .ok_or(XPresentationPropertyError::Atom(
                XAtomError::AtomSpaceExhausted,
            ))
    };
    let net_wm_state = atom(X_ATOM_NAME_NET_WM_STATE)?;
    let fullscreen = atom(X_ATOM_NAME_NET_WM_STATE_FULLSCREEN)?;
    let hidden = atom(X_ATOM_NAME_NET_WM_STATE_HIDDEN)?;
    let maximized_horz = atom(X_ATOM_NAME_NET_WM_STATE_MAXIMIZED_HORZ)?;
    let maximized_vert = atom(X_ATOM_NAME_NET_WM_STATE_MAXIMIZED_VERT)?;
    let wm_state = atom(X_ATOM_NAME_WM_STATE)?;

    let encode = |value: u32| match byte_order {
        XByteOrder::LittleEndian => value.to_le_bytes(),
        XByteOrder::BigEndian => value.to_be_bytes(),
    };
    let mut net_state_bytes = Vec::with_capacity(16);
    if state.fullscreen {
        net_state_bytes.extend_from_slice(&encode(fullscreen));
    }
    if state.maximized {
        net_state_bytes.extend_from_slice(&encode(maximized_vert));
        net_state_bytes.extend_from_slice(&encode(maximized_horz));
    }
    if state.minimized {
        net_state_bytes.extend_from_slice(&encode(hidden));
    }
    let mut wm_state_bytes = Vec::with_capacity(8);
    // ICCCM NormalState=1 and IconicState=3; the icon window is None.
    wm_state_bytes.extend_from_slice(&encode(if state.minimized { 3 } else { 1 }));
    wm_state_bytes.extend_from_slice(&encode(0));

    let changes = [
        (net_wm_state, X_ATOM_ATOM, net_state_bytes),
        (wm_state, wm_state, wm_state_bytes),
    ];
    let current_total = properties
        .records
        .values()
        .try_fold(0usize, |total, record| {
            total.checked_add(record.bytes.len())
        })
        .ok_or(XPropertyError::TableTooLarge {
            len: usize::MAX,
            max: X_PROPERTY_MAX_TABLE_BYTES,
        })?;
    let previous_total = changes.iter().try_fold(0usize, |total, (property, _, _)| {
        total.checked_add(
            properties
                .records
                .get(&(namespace, window, *property))
                .map_or(0, |record| record.bytes.len()),
        )
    });
    let replacement_total = changes.iter().try_fold(0usize, |total, (_, _, bytes)| {
        total.checked_add(bytes.len())
    });
    let table_len = previous_total
        .and_then(|previous| current_total.checked_sub(previous))
        .and_then(|retained| replacement_total.and_then(|added| retained.checked_add(added)))
        .ok_or(XPropertyError::TableTooLarge {
            len: usize::MAX,
            max: X_PROPERTY_MAX_TABLE_BYTES,
        })?;
    if table_len > X_PROPERTY_MAX_TABLE_BYTES {
        return Err(XPropertyError::TableTooLarge {
            len: table_len,
            max: X_PROPERTY_MAX_TABLE_BYTES,
        }
        .into());
    }

    let mut changed = Vec::with_capacity(changes.len());
    for (property, property_type, bytes) in changes {
        let key = (namespace, window, property);
        let previous = properties.records.get(&key);
        let value_changed = previous.is_none_or(|record| {
            record.property_type != property_type || record.format != 32 || record.bytes != bytes
        });
        properties.engine_owned.insert(key);
        if !value_changed {
            continue;
        }
        changed.push(property);
        let generation = previous.map_or(1, |record| record.generation.saturating_add(1));
        properties.records.insert(
            key,
            XPropertyRecord {
                namespace,
                window,
                property,
                property_type,
                format: 32,
                bytes,
                generation,
            },
        );
    }
    Ok(changed)
}

/// Every EWMH hint Sophia acts on, and nothing else.
///
/// This is a claim, and a client that believes a hint works and finds it ignored
/// is worse off than one told plainly that it does not. Each entry here has
/// behaviour behind it: the states and window types are read back into layout
/// facts, the struts become output reservations, and the name reaches metadata
/// disclosure, and `_NET_ACTIVE_WINDOW` is written from the authority's own
/// input focus every time it changes (`publish_active_window`). Hints Sophia
/// merely knows the name of are deliberately absent -- `_NET_CLIENT_LIST`,
/// `_NET_CURRENT_DESKTOP`, `_NET_FRAME_EXTENTS`, `_NET_WM_SYNC_REQUEST` and
/// `_NET_WM_MOVERESIZE` among them, several of which clients do ask about.
pub const X_EWMH_SUPPORTED_ATOM_NAMES: &[&str] = &[
    X_ATOM_NAME_NET_SUPPORTING_WM_CHECK,
    X_ATOM_NAME_NET_ACTIVE_WINDOW,
    X_ATOM_NAME_NET_WM_NAME,
    X_ATOM_NAME_NET_WM_STATE,
    X_ATOM_NAME_NET_WM_STATE_FULLSCREEN,
    X_ATOM_NAME_NET_WM_STATE_HIDDEN,
    X_ATOM_NAME_NET_WM_STATE_MAXIMIZED_HORZ,
    X_ATOM_NAME_NET_WM_STATE_MAXIMIZED_VERT,
    X_ATOM_NAME_NET_WM_STRUT,
    X_ATOM_NAME_NET_WM_STRUT_PARTIAL,
    X_ATOM_NAME_NET_WM_WINDOW_TYPE,
];

/// The name the check window reports as the window manager.
///
/// The authority synthesizes this advertisement, and the blind policy that
/// places windows speaks no X11 -- it could not own the window a client reads
/// this from. Naming the authority also survives replacing that policy.
pub const X_EWMH_WM_NAME: &str = "Sophia";

/// Publishes the answer to "is a window manager running?".
///
/// A client reads `_NET_SUPPORTING_WM_CHECK` from the root, reads it again from
/// the window that names, and treats the pair as proof a manager is live -- the
/// self-reference is what distinguishes a running manager from a stale root
/// property left behind by a dead one. Without it a toolkit concludes there is
/// no manager and takes an unmanaged path, while Sophia goes on configuring and
/// placing its windows underneath it.
///
/// Written under `Replace`, so calling this again for a namespace already seeded
/// costs a comparison and changes nothing.
pub fn seed_wm_advertisement(
    properties: &mut XPropertyTable,
    atoms: &mut XAtomTable,
    namespace: NamespaceId,
    byte_order: XByteOrder,
) -> Result<(), XPresentationPropertyError> {
    if !namespace.is_valid() {
        return Err(XPropertyError::InvalidNamespace.into());
    }
    let mut atom = |name: &str| -> Result<XAtom, XPresentationPropertyError> {
        atoms
            .intern(name, false)?
            .ok_or(XPresentationPropertyError::Atom(
                XAtomError::AtomSpaceExhausted,
            ))
    };
    let supporting = atom(X_ATOM_NAME_NET_SUPPORTING_WM_CHECK)?;
    let supported = atom(X_ATOM_NAME_NET_SUPPORTED)?;
    let net_wm_name = atom(X_ATOM_NAME_NET_WM_NAME)?;
    let utf8 = atom(X_ATOM_NAME_UTF8_STRING)?;
    let mut supported_bytes = Vec::with_capacity(X_EWMH_SUPPORTED_ATOM_NAMES.len() * 4);
    for name in X_EWMH_SUPPORTED_ATOM_NAMES {
        supported_bytes.extend_from_slice(&encode_property_u32(byte_order, atom(name)?));
    }

    let root = XResourceId::new(u64::from(crate::X_SETUP_DEFAULT_ROOT), 1);
    let check = XResourceId::new(u64::from(crate::X_SETUP_WM_CHECK_WINDOW), 1);
    let check_bytes = encode_property_u32(byte_order, crate::X_SETUP_WM_CHECK_WINDOW).to_vec();
    let mut changes = vec![
        (root, supporting, X_ATOM_WINDOW, 32u8, check_bytes.clone()),
        (root, supported, X_ATOM_ATOM, 32, supported_bytes),
        (check, supporting, X_ATOM_WINDOW, 32, check_bytes),
        (
            check,
            net_wm_name,
            utf8,
            8,
            X_EWMH_WM_NAME.as_bytes().to_vec(),
        ),
    ];
    // SEEDED AS NONE, ONLY WHILE ABSENT. The advertisement is re-seeded by
    // every connection, and this one entry is state rather than a claim: a
    // later client's seeding must not put the focus back to nothing. What
    // matters at seeding is that the property exists with the right type --
    // a toolkit that finds it absent reports the missing type by name, and
    // asking for the name of None is answered by exiting the client.
    let active = atom(X_ATOM_NAME_NET_ACTIVE_WINDOW)?;
    if !properties.records.contains_key(&(namespace, root, active)) {
        changes.push((root, active, X_ATOM_WINDOW, 32, vec![0; 4]));
    }

    let added = changes
        .iter()
        .try_fold(0usize, |total, (_, _, _, _, bytes)| {
            total.checked_add(bytes.len())
        })
        .ok_or(XPropertyError::TableTooLarge {
            len: usize::MAX,
            max: X_PROPERTY_MAX_TABLE_BYTES,
        })?;
    if added > X_PROPERTY_MAX_TABLE_BYTES {
        return Err(XPropertyError::TableTooLarge {
            len: added,
            max: X_PROPERTY_MAX_TABLE_BYTES,
        }
        .into());
    }

    for (window, property, property_type, format, bytes) in changes {
        write_engine_owned(
            properties,
            namespace,
            window,
            property,
            property_type,
            format,
            bytes,
        );
    }
    Ok(())
}

/// Publishes which window holds the input focus, as `_NET_ACTIVE_WINDOW` on
/// the root.
///
/// Behaviour behind the advertisement rather than a name: written from the
/// authority's own input focus every time it changes, by whichever layer
/// holds the property table -- request dispatch after each request, the
/// control writer after each session focus command. `None`, `PointerRoot`
/// and the root itself are published as 0, which is EWMH for no active
/// window. Owned by the authority like the rest of the advertisement, so a
/// client may read it and may not replace it, and written under `Replace`,
/// so publishing the same focus twice changes nothing.
pub fn publish_active_window(
    properties: &mut XPropertyTable,
    atoms: &mut XAtomTable,
    namespace: NamespaceId,
    byte_order: XByteOrder,
    window: u32,
) -> Result<(), XPresentationPropertyError> {
    if !namespace.is_valid() {
        return Err(XPropertyError::InvalidNamespace.into());
    }
    let active = atoms.intern(X_ATOM_NAME_NET_ACTIVE_WINDOW, false)?.ok_or(
        XPresentationPropertyError::Atom(XAtomError::AtomSpaceExhausted),
    )?;
    let root = XResourceId::new(u64::from(crate::X_SETUP_DEFAULT_ROOT), 1);
    write_engine_owned(
        properties,
        namespace,
        root,
        active,
        X_ATOM_WINDOW,
        32,
        encode_property_u32(byte_order, window).to_vec(),
    );
    Ok(())
}

/// One authority-owned property, replaced only when its value changed.
fn write_engine_owned(
    properties: &mut XPropertyTable,
    namespace: NamespaceId,
    window: XResourceId,
    property: XAtom,
    property_type: XAtom,
    format: u8,
    bytes: Vec<u8>,
) {
    let key = (namespace, window, property);
    let previous = properties.records.get(&key);
    // Authority-owned: a client may read these and must not replace them.
    properties.engine_owned.insert(key);
    if previous.is_some_and(|record| {
        record.property_type == property_type && record.format == format && record.bytes == bytes
    }) {
        return;
    }
    let generation = previous.map_or(1, |record| record.generation.saturating_add(1));
    properties.records.insert(
        key,
        XPropertyRecord {
            namespace,
            window,
            property,
            property_type,
            format,
            bytes,
            generation,
        },
    );
}

fn encode_property_u32(byte_order: XByteOrder, value: u32) -> [u8; 4] {
    match byte_order {
        XByteOrder::LittleEndian => value.to_le_bytes(),
        XByteOrder::BigEndian => value.to_be_bytes(),
    }
}

pub(crate) fn read_property_value(
    property_type: XAtom,
    format: u8,
    bytes: &[u8],
    requested_type: XAtom,
    long_offset: u32,
    long_length: u32,
) -> Result<XPropertyReadReply, XPropertyError> {
    if requested_type != X_PROPERTY_ANY_TYPE && requested_type != property_type {
        return Ok(XPropertyReadReply {
            property_type,
            format,
            bytes_after: u32::try_from(bytes.len()).unwrap_or(u32::MAX),
            item_count: 0,
            bytes: Vec::new(),
        });
    }

    let offset = usize::try_from(long_offset)
        .ok()
        .and_then(|value| value.checked_mul(4))
        .ok_or(XPropertyError::InvalidOffset)?;
    if offset > bytes.len() {
        return Err(XPropertyError::InvalidOffset);
    }

    let remaining = bytes.len() - offset;
    let requested_bytes = usize::try_from(long_length)
        .unwrap_or(usize::MAX)
        .saturating_mul(4);
    let returned_len = remaining.min(requested_bytes);
    let bytes_after = remaining - returned_len;
    let item_width = usize::from(format / 8);
    Ok(XPropertyReadReply {
        property_type,
        format,
        bytes_after: u32::try_from(bytes_after).unwrap_or(u32::MAX),
        item_count: u32::try_from(returned_len / item_width).unwrap_or(u32::MAX),
        bytes: bytes[offset..offset + returned_len].to_vec(),
    })
}

pub(crate) fn validate_property_format(format: u8) -> Result<(), XPropertyError> {
    match format {
        8 | 16 | 32 => Ok(()),
        other => Err(XPropertyError::InvalidFormat(other)),
    }
}

fn ensure_same_property_shape(
    record: &XPropertyRecord,
    change: &XPropertyChange,
) -> Result<(), XPropertyError> {
    if record.property_type != change.property_type || record.format != change.format {
        return Err(XPropertyError::TypeMismatch);
    }
    Ok(())
}

fn joined_bytes(first: &[u8], second: &[u8]) -> Result<Vec<u8>, XPropertyError> {
    let len = first
        .len()
        .checked_add(second.len())
        .ok_or(XPropertyError::ValueTooLarge {
            len: usize::MAX,
            max: X_PROPERTY_MAX_VALUE_BYTES,
        })?;
    if len > X_PROPERTY_MAX_VALUE_BYTES {
        return Err(XPropertyError::ValueTooLarge {
            len,
            max: X_PROPERTY_MAX_VALUE_BYTES,
        });
    }
    let mut bytes = Vec::with_capacity(len);
    bytes.extend_from_slice(first);
    bytes.extend_from_slice(second);
    Ok(bytes)
}

include!("property/atom_sweep.rs");

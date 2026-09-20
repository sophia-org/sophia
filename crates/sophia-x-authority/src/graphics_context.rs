use std::collections::BTreeMap;

use sophia_protocol::{NamespaceId, Rect};

use crate::{XAuthorityAccessError, XFontHandle, XResourceId};

pub const X_GX_COPY: u8 = 3;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct XPoint {
    pub x: i16,
    pub y: i16,
}

/// Graphics context defaults, named rather than spelled as numbers.
pub const X_LINE_SOLID: u8 = 0;
pub const X_LINE_ON_OFF_DASH: u8 = 1;
pub const X_LINE_DOUBLE_DASH: u8 = 2;
pub const X_CAP_NOT_LAST: u8 = 0;
pub const X_CAP_BUTT: u8 = 1;
pub const X_CAP_ROUND: u8 = 2;
pub const X_CAP_PROJECTING: u8 = 3;
pub const X_JOIN_MITER: u8 = 0;
pub const X_JOIN_ROUND: u8 = 1;
pub const X_JOIN_BEVEL: u8 = 2;
pub const X_FILL_SOLID: u8 = 0;
pub const X_FILL_TILED: u8 = 1;
pub const X_FILL_STIPPLED: u8 = 2;
pub const X_FILL_OPAQUE_STIPPLED: u8 = 3;
pub const X_FILL_EVEN_ODD: u8 = 0;
pub const X_FILL_WINDING: u8 = 1;
pub const X_CLIP_BY_CHILDREN: u8 = 0;
pub const X_INCLUDE_INFERIORS: u8 = 1;
pub const X_ARC_CHORD: u8 = 0;
pub const X_ARC_PIE_SLICE: u8 = 1;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct XGraphicsContextValues {
    pub function: u8,
    pub plane_mask: u32,
    pub foreground: u32,
    pub background: u32,
    pub line_width: u16,
    /// Solid, on-off dashed, or double dashed.
    pub line_style: u8,
    /// How a line's ends are drawn: not-last, butt, round, projecting.
    pub cap_style: u8,
    /// How a polyline's corners are drawn: miter, round, bevel.
    pub join_style: u8,
    pub fill_style: u8,
    /// Even-odd or winding, for a self-intersecting filled polygon.
    pub fill_rule: u8,
    pub tile: Option<XResourceId>,
    pub stipple: Option<XResourceId>,
    pub tile_stipple_x_origin: i16,
    pub tile_stipple_y_origin: i16,
    pub subwindow_mode: u8,
    pub dash_offset: u16,
    /// The dash pattern, alternating on and off runs. An odd-length pattern is
    /// doubled when it is set, as the protocol requires.
    pub dashes: Vec<u8>,
    /// Whether a filled arc is closed by a chord or through its centre.
    pub arc_mode: u8,
    pub font: Option<XResourceId>,
    pub graphics_exposures: bool,
    pub clip_x_origin: i16,
    pub clip_y_origin: i16,
    /// None is unrestricted; an explicitly empty list suppresses all drawing.
    pub clip_rectangles: Option<Vec<Rect>>,
    /// A requested pixmap clip; unsupported values are rejected before storage.
    pub clip_mask: Option<XResourceId>,
}

impl Default for XGraphicsContextValues {
    fn default() -> Self {
        Self {
            function: X_GX_COPY,
            plane_mask: u32::MAX,
            foreground: 0,
            background: 1,
            line_width: 0,
            line_style: X_LINE_SOLID,
            cap_style: X_CAP_BUTT,
            join_style: X_JOIN_MITER,
            fill_style: 0,
            fill_rule: X_FILL_EVEN_ODD,
            tile: None,
            stipple: None,
            tile_stipple_x_origin: 0,
            tile_stipple_y_origin: 0,
            subwindow_mode: X_CLIP_BY_CHILDREN,
            dash_offset: 0,
            dashes: vec![4, 4],
            arc_mode: X_ARC_PIE_SLICE,
            font: None,
            graphics_exposures: true,
            clip_x_origin: 0,
            clip_y_origin: 0,
            clip_rectangles: None,
            clip_mask: None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct XGraphicsContextRecord {
    pub id: XResourceId,
    pub drawable: XResourceId,
    pub depth: u8,
    pub namespace: NamespaceId,
    pub values: XGraphicsContextValues,
    pub(crate) font_face: XFontHandle,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct XGraphicsContextTable {
    records: BTreeMap<XResourceId, XGraphicsContextRecord>,
}

impl XGraphicsContextTable {
    pub fn create(
        &mut self,
        namespace: NamespaceId,
        id: XResourceId,
        drawable: XResourceId,
        depth: u8,
        values: XGraphicsContextValues,
        font_face: XFontHandle,
    ) -> Result<(), XAuthorityAccessError> {
        if !namespace.is_valid() {
            return Err(XAuthorityAccessError::InvalidNamespace);
        }
        if !id.is_valid() || !drawable.is_valid() {
            return Err(XAuthorityAccessError::InvalidResource);
        }
        if self.records.contains_key(&id) {
            return Err(XAuthorityAccessError::InvalidResource);
        }
        self.records.insert(
            id,
            XGraphicsContextRecord {
                id,
                drawable,
                depth,
                namespace,
                values,
                font_face,
            },
        );
        Ok(())
    }

    pub(crate) fn contains(&self, id: XResourceId) -> bool {
        self.records.contains_key(&id)
    }

    pub fn get(
        &self,
        namespace: NamespaceId,
        id: XResourceId,
    ) -> Result<&XGraphicsContextRecord, XAuthorityAccessError> {
        let record = self
            .records
            .get(&id)
            .ok_or(XAuthorityAccessError::UnknownResource)?;
        if record.namespace != namespace {
            return Err(XAuthorityAccessError::CrossNamespaceDenied);
        }
        Ok(record)
    }

    pub fn change(
        &mut self,
        namespace: NamespaceId,
        id: XResourceId,
        mask: u32,
        values: XGraphicsContextValues,
        font_face: Option<XFontHandle>,
    ) -> Result<(), XAuthorityAccessError> {
        let record = self
            .records
            .get_mut(&id)
            .ok_or(XAuthorityAccessError::UnknownResource)?;
        if record.namespace != namespace {
            return Err(XAuthorityAccessError::CrossNamespaceDenied);
        }
        if mask & (1 << 19) != 0 {
            // A clip mask and a rectangle list are two spellings of one
            // component, so setting either clears the other.
            record.values.clip_rectangles = None;
            record.values.clip_mask = values.clip_mask;
        }
        if mask & (1 << 0) != 0 {
            record.values.function = values.function;
        }
        if mask & (1 << 1) != 0 {
            record.values.plane_mask = values.plane_mask;
        }
        if mask & (1 << 2) != 0 {
            record.values.foreground = values.foreground;
        }
        if mask & (1 << 3) != 0 {
            record.values.background = values.background;
        }
        if mask & (1 << 4) != 0 {
            record.values.line_width = values.line_width;
        }
        if mask & (1 << 5) != 0 {
            record.values.line_style = values.line_style;
        }
        if mask & (1 << 6) != 0 {
            record.values.cap_style = values.cap_style;
        }
        if mask & (1 << 7) != 0 {
            record.values.join_style = values.join_style;
        }
        if mask & (1 << 8) != 0 {
            record.values.fill_style = values.fill_style;
        }
        if mask & (1 << 9) != 0 {
            record.values.fill_rule = values.fill_rule;
        }
        if mask & (1 << 10) != 0 {
            record.values.tile = values.tile;
        }
        if mask & (1 << 11) != 0 {
            record.values.stipple = values.stipple;
        }
        if mask & (1 << 12) != 0 {
            record.values.tile_stipple_x_origin = values.tile_stipple_x_origin;
        }
        if mask & (1 << 13) != 0 {
            record.values.tile_stipple_y_origin = values.tile_stipple_y_origin;
        }
        if mask & (1 << 14) != 0 {
            record.values.font = values.font;
            record.font_face = font_face.expect("a validated GC font accompanies the font mask");
        }
        if mask & (1 << 16) != 0 {
            record.values.graphics_exposures = values.graphics_exposures;
        }
        if mask & (1 << 17) != 0 {
            record.values.clip_x_origin = values.clip_x_origin;
        }
        if mask & (1 << 15) != 0 {
            record.values.subwindow_mode = values.subwindow_mode;
        }
        if mask & (1 << 18) != 0 {
            record.values.clip_y_origin = values.clip_y_origin;
        }
        if mask & (1 << 20) != 0 {
            record.values.dash_offset = values.dash_offset;
        }
        if mask & (1 << 21) != 0 {
            record.values.dashes = values.dashes.clone();
        }
        if mask & (1 << 22) != 0 {
            record.values.arc_mode = values.arc_mode;
        }
        Ok(())
    }

    /// Copy the named components of one graphics context onto another.
    ///
    /// Both must exist and belong to the caller; a component the mask does not
    /// name is left as the destination had it.
    pub fn copy(
        &mut self,
        namespace: NamespaceId,
        source: XResourceId,
        destination: XResourceId,
        mask: u32,
    ) -> Result<(), XAuthorityAccessError> {
        let values = {
            let record = self.get(namespace, source)?;
            (record.values.clone(), record.font_face.clone())
        };
        if source == destination {
            // Copying a context onto itself is legal and changes nothing.
            let _ = self.get(namespace, destination)?;
            return Ok(());
        }
        self.change(namespace, destination, mask, values.0, Some(values.1))
    }

    /// Replace a graphics context's dash pattern.
    ///
    /// An odd-length pattern is doubled, so a pattern of one length alternates
    /// evenly rather than describing only its on run.
    pub fn set_dashes(
        &mut self,
        namespace: NamespaceId,
        id: XResourceId,
        dash_offset: u16,
        dashes: &[u8],
    ) -> Result<(), XAuthorityAccessError> {
        let record = self
            .records
            .get_mut(&id)
            .ok_or(XAuthorityAccessError::UnknownResource)?;
        if record.namespace != namespace {
            return Err(XAuthorityAccessError::CrossNamespaceDenied);
        }
        let mut pattern = dashes.to_vec();
        if !pattern.len().is_multiple_of(2) {
            pattern.extend_from_within(..);
        }
        record.values.dash_offset = dash_offset;
        record.values.dashes = pattern;
        Ok(())
    }

    pub fn set_clip_rectangles(
        &mut self,
        namespace: NamespaceId,
        id: XResourceId,
        clip_x_origin: i16,
        clip_y_origin: i16,
        rectangles: Vec<Rect>,
    ) -> Result<(), XAuthorityAccessError> {
        let record = self
            .records
            .get_mut(&id)
            .ok_or(XAuthorityAccessError::UnknownResource)?;
        if record.namespace != namespace {
            return Err(XAuthorityAccessError::CrossNamespaceDenied);
        }
        record.values.clip_x_origin = clip_x_origin;
        record.values.clip_y_origin = clip_y_origin;
        record.values.clip_rectangles = Some(rectangles);
        Ok(())
    }

    pub fn remove(
        &mut self,
        namespace: NamespaceId,
        id: XResourceId,
    ) -> Result<(), XAuthorityAccessError> {
        self.get(namespace, id)?;
        self.records.remove(&id);
        Ok(())
    }

    pub fn ids_for_namespace_in_client_range(
        &self,
        namespace: NamespaceId,
        range: crate::XWireClientResourceRange,
    ) -> Vec<XResourceId> {
        self.records
            .values()
            .filter(|record| {
                record.namespace == namespace
                    && u32::try_from(record.id.local.raw())
                        .is_ok_and(|raw| range.owns_new_resource(raw))
            })
            .map(|record| record.id)
            .collect()
    }
}

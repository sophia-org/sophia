use std::collections::BTreeMap;

use sophia_protocol::{NamespaceId, Rect};

use crate::{XAuthorityAccessError, XFontHandle, XResourceId};

pub const X_GX_COPY: u8 = 3;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct XPoint {
    pub x: i16,
    pub y: i16,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct XGraphicsContextValues {
    pub function: u8,
    pub plane_mask: u32,
    pub foreground: u32,
    pub background: u32,
    pub line_width: u16,
    pub fill_style: u8,
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
            fill_style: 0,
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
        if values.clip_mask.is_some() {
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
        if values.clip_mask.is_some() {
            return Err(XAuthorityAccessError::InvalidResource);
        }
        if mask & (1 << 19) != 0 {
            record.values.clip_rectangles = None;
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
        if mask & (1 << 8) != 0 {
            record.values.fill_style = values.fill_style;
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
        if mask & (1 << 18) != 0 {
            record.values.clip_y_origin = values.clip_y_origin;
        }
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

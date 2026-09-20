impl XAuthorityRuntime {
    pub fn create_graphics_context(
        &mut self,
        namespace: NamespaceId,
        gc: crate::XResourceId,
        drawable: crate::XResourceId,
        values: XGraphicsContextValues,
    ) -> Result<(), XAuthorityRuntimeError> {
        self.validate_drawable_access(namespace, drawable)?;
        let depth = self.drawable_depth(namespace, drawable)?;
        // A graphics context with no font named still draws text, so it holds
        // the built-in face rather than an empty one. `Default` for a face is
        // a font with no glyphs at all, which would silently draw nothing.
        let font_face = values
            .font
            .map(|font| self.font_face(namespace, font))
            .transpose()?
            .unwrap_or_else(crate::builtin_font_handle);
        self.graphics_contexts
            .create(namespace, gc, drawable, depth, values, font_face)
            .map_err(XAuthorityRuntimeError::from)?;
        Ok(())
    }

    pub fn graphics_context_values(
        &self,
        namespace: NamespaceId,
        gc: crate::XResourceId,
    ) -> Result<XGraphicsContextValues, XAuthorityRuntimeError> {
        self.graphics_contexts
            .get(namespace, gc)
            .map(core_draw_gc_values)
            .map_err(Into::into)
    }

    pub(crate) fn graphics_context_depth_and_values(
        &self,
        namespace: NamespaceId,
        gc: crate::XResourceId,
    ) -> Result<(u8, XGraphicsContextValues), XAuthorityRuntimeError> {
        self.graphics_contexts
            .get(namespace, gc)
            .map(|record| (record.depth, core_draw_gc_values(record)))
            .map_err(Into::into)
    }

    /// Whether a graphics context exists and belongs to this namespace.
    pub fn validate_graphics_context(
        &self,
        namespace: NamespaceId,
        gc: crate::XResourceId,
    ) -> Result<(), XAuthorityRuntimeError> {
        self.graphics_contexts.get(namespace, gc).map(|_| ()).map_err(Into::into)
    }

    /// Copy graphics context components, as `CopyGC` asks.
    pub fn copy_graphics_context(
        &mut self,
        namespace: NamespaceId,
        source: crate::XResourceId,
        destination: crate::XResourceId,
        mask: u32,
    ) -> Result<(), XAuthorityRuntimeError> {
        self.graphics_contexts
            .copy(namespace, source, destination, mask)
            .map_err(Into::into)
    }

    /// Replace a graphics context's dash pattern, as `SetDashes` asks.
    pub fn set_graphics_context_dashes(
        &mut self,
        namespace: NamespaceId,
        gc: crate::XResourceId,
        dash_offset: u16,
        dashes: &[u8],
    ) -> Result<(), XAuthorityRuntimeError> {
        self.graphics_contexts
            .set_dashes(namespace, gc, dash_offset, dashes)
            .map_err(Into::into)
    }

    pub(crate) fn graphics_context_depth_values_and_font(
        &self,
        namespace: NamespaceId,
        gc: crate::XResourceId,
    ) -> Result<(u8, XGraphicsContextValues, XFontHandle), XAuthorityRuntimeError> {
        self.graphics_contexts
            .get(namespace, gc)
            .map(|record| (record.depth, core_draw_gc_values(record), record.font_face.clone()))
            .map_err(Into::into)
    }

    pub(crate) fn fontable_face(
        &self,
        namespace: NamespaceId,
        fontable: crate::XResourceId,
    ) -> Result<XFontHandle, XAuthorityRuntimeError> {
        match self.font_face(namespace, fontable) {
            Ok(face) => Ok(face),
            Err(
                XAuthorityRuntimeError::UnknownResource | XAuthorityRuntimeError::WrongResourceKind,
            ) => self
                .graphics_contexts
                .get(namespace, fontable)
                .map(|record| record.font_face.clone())
                .map_err(Into::into),
            Err(error) => Err(error),
        }
    }

    pub fn change_graphics_context(
        &mut self,
        namespace: NamespaceId,
        gc: crate::XResourceId,
        mask: u32,
        values: XGraphicsContextValues,
    ) -> Result<(), XAuthorityRuntimeError> {
        let font_face = if mask & (1 << 14) != 0 {
            Some(self.font_face(
                namespace,
                values.font.ok_or(XAuthorityRuntimeError::InvalidResource)?,
            )?)
        } else {
            None
        };
        self.graphics_contexts
            .change(namespace, gc, mask, values, font_face)
            .map_err(Into::into)
    }

    pub fn set_graphics_context_clip_rectangles(
        &mut self,
        namespace: NamespaceId,
        gc: crate::XResourceId,
        clip_x_origin: i16,
        clip_y_origin: i16,
        rectangles: Vec<Rect>,
    ) -> Result<(), XAuthorityRuntimeError> {
        self.graphics_contexts
            .set_clip_rectangles(namespace, gc, clip_x_origin, clip_y_origin, rectangles)
            .map_err(Into::into)
    }

    pub fn free_graphics_context(
        &mut self,
        namespace: NamespaceId,
        gc: crate::XResourceId,
    ) -> Result<(), XAuthorityRuntimeError> {
        self.graphics_contexts
            .remove(namespace, gc)
            .map_err(Into::into)
    }
}

fn core_draw_gc_values(record: &crate::XGraphicsContextRecord) -> XGraphicsContextValues {
    let mut values = record.values.clone();
    // The GC depth, rather than the CPU storage format, defines writable planes.
    values.plane_mask &= 32_u32
        .checked_sub(u32::from(record.depth))
        .and_then(|shift| u32::MAX.checked_shr(shift))
        .unwrap_or(0);
    values
}

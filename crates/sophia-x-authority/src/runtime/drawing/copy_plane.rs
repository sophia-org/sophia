// The `CopyPlane` request, from validation to damage. Included by runtime.rs
// beside the other drawing paths, so it shares their imports.

impl XAuthorityRuntime {
    /// Copy one plane of a source drawable, expanding it through the
    /// graphics context's foreground and background.
    pub fn apply_copy_plane(
        &mut self,
        transaction: TransactionId,
        namespace: NamespaceId,
        source: crate::XResourceId,
        destination: crate::XResourceId,
        source_origin: (i32, i32),
        destination_origin: (i32, i32),
        extent: (i32, i32),
        bit_plane: u32,
        gc: &XGraphicsContextValues,
    ) -> XAuthorityResponsePacket {
        if let Err(error) = self.validate_drawable_access(namespace, source) {
            return XAuthorityResponsePacket::rejected(transaction, error);
        }
        let source = self.draw_key(namespace, source);
        let (destination, size, window_generation) =
            match self.draw_target(namespace, destination) {
            Ok(target) => target,
            Err(error) => return XAuthorityResponsePacket::rejected(transaction, error),
        };
        let Some((update, damage)) = self.software_buffers.copy_plane(
            source,
            destination,
            size,
            source_origin,
            destination_origin,
            extent,
            bit_plane,
            gc,
        ) else {
            return XAuthorityResponsePacket::accepted(transaction);
        };
        let Some(generation) = window_generation else {
            return XAuthorityResponsePacket::accepted(transaction);
        };
        let handle = update.handle();
        self.finish_drawing_update(XDrawingUpdate::core_draw(
            transaction,
            namespace,
            destination,
            handle,
            Region::single(damage),
            generation,
            250,
        ))
    }

}

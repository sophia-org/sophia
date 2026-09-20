//! Expanding one plane of a drawable into colour.
//!
//! A depth-one bitmap carries a shape, not a colour. `CopyPlane` selects one
//! bit of a source and paints the graphics context's foreground where it is
//! set and the background where it is clear, which is how every Xaw button
//! icon and menu checkmark reaches the screen.

use super::{
    XAuthorityCpuDrawResult, XGraphicsContextValues, XResourceId, XSoftwareBufferStore,
    finish_immutable_update, raster_ops::fill_rect,
};
use sophia_protocol::{Rect, Size};

impl XSoftwareBufferStore {
    /// Copy one plane of a source drawable into a destination.
    ///
    /// The selected bit becomes a mask: where it is set the destination takes
    /// the graphics context's foreground, and where it is clear the
    /// background. That is how a depth-one bitmap becomes coloured pixels,
    /// which is the path every Xaw button icon and menu checkmark takes.
    ///
    /// Emitted as horizontal runs rather than one rectangle per pixel, so a
    /// wide bitmap costs spans proportional to its content rather than to its
    /// area.
    pub fn copy_plane(
        &mut self,
        source: XResourceId,
        destination: XResourceId,
        size: Size,
        source_origin: (i32, i32),
        destination_origin: (i32, i32),
        extent: (i32, i32),
        bit_plane: u32,
        gc: &XGraphicsContextValues,
    ) -> Option<(XAuthorityCpuDrawResult, Rect)> {
        let source_pixels = self.buffers.get(&source).cloned()?;
        let handle = self.allocate_handle();
        let (buffer, replaced) = self.ensure(destination, size, handle)?;
        let stride = usize::try_from(source_pixels.stride).unwrap_or(0);
        for row in 0..extent.1 {
            let mut run: Option<(i32, bool)> = None;
            for column in 0..=extent.0 {
                // One past the end closes the final run.
                let set = if column == extent.0 {
                    None
                } else {
                    let x = source_origin.0 + column;
                    let y = source_origin.1 + row;
                    if x < 0
                        || y < 0
                        || x >= source_pixels.size.width
                        || y >= source_pixels.size.height
                    {
                        None
                    } else {
                        let offset = usize::try_from(y).unwrap_or(0) * stride
                            + usize::try_from(x).unwrap_or(0) * 4;
                        source_pixels.bytes.get(offset..offset + 4).map(|bytes| {
                            u32::from_le_bytes(bytes.try_into().unwrap_or([0; 4])) & bit_plane != 0
                        })
                    }
                };
                match (run, set) {
                    (None, Some(value)) => run = Some((column, value)),
                    (Some((_, value)), Some(current)) if current == value => {}
                    (Some((start, value)), current) => {
                        let pixel = if value { gc.foreground } else { gc.background };
                        fill_rect(
                            buffer,
                            Rect {
                                x: destination_origin.0 + start,
                                y: destination_origin.1 + row,
                                width: column - start,
                                height: 1,
                            },
                            pixel,
                            gc,
                        );
                        run = current.map(|value| (column, value));
                    }
                    (None, None) => {}
                }
            }
        }
        let damage = Rect {
            x: destination_origin.0,
            y: destination_origin.1,
            width: extent.0,
            height: extent.1,
        };
        let published = Some(damage);
        let result = finish_immutable_update(buffer, handle, replaced, published);
        self.note_export_damage(destination, replaced, published);
        Some((result?, damage))
    }
}

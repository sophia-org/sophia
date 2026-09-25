use super::*;

/// One immutable shell resource placed in output-local physical pixels.
/// The lease is carried through every native frame clone so resource release
/// cannot precede the last scanout reference.
#[derive(Clone)]
pub struct CompositorContentImage {
    pub node: CompositorNodeId,
    pub generation: u64,
    pub output_size_px: Size,
    pub geometry_px: Rect,
    pub size_px: Size,
    pub stride: u32,
    pub format: u32,
    pub resource: sophia_runtime::ContentResourceLease,
}

impl core::fmt::Debug for CompositorContentImage {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("CompositorContentImage")
            .field("node", &self.node)
            .field("generation", &self.generation)
            .field("output_size_px", &self.output_size_px)
            .field("geometry_px", &self.geometry_px)
            .field("size_px", &self.size_px)
            .field("stride", &self.stride)
            .field("format", &self.format)
            .finish_non_exhaustive()
    }
}

impl PartialEq for CompositorContentImage {
    fn eq(&self, other: &Self) -> bool {
        self.node == other.node
            && self.generation == other.generation
            && self.output_size_px == other.output_size_px
            && self.geometry_px == other.geometry_px
            && self.size_px == other.size_px
            && self.stride == other.stride
            && self.format == other.format
            && self.resource.description() == other.resource.description()
    }
}

impl Eq for CompositorContentImage {}

/// Non-owning facts used by damage and presented-input history. No pixel
/// allocation or renderer lease can be retained by this record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompositorContentIdentity {
    pub node: CompositorNodeId,
    pub generation: u64,
    pub output_size_px: Size,
    pub geometry_px: Rect,
    pub size_px: Size,
    pub stride: u32,
    pub format: u32,
    pub resource: sophia_protocol::ContentResourceBegin,
    pub source_bytes: usize,
}

/// Metadata common to a live composition source and its non-owning history.
pub trait CompositorContentMetadata {
    fn content_identity(&self) -> CompositorContentIdentity;
}

impl CompositorContentMetadata for CompositorContentImage {
    fn content_identity(&self) -> CompositorContentIdentity {
        CompositorContentIdentity {
            node: self.node,
            generation: self.generation,
            output_size_px: self.output_size_px,
            geometry_px: self.geometry_px,
            size_px: self.size_px,
            stride: self.stride,
            format: self.format,
            resource: self.resource.description().clone(),
            source_bytes: self.resource.bytes().len(),
        }
    }
}

impl CompositorContentMetadata for CompositorContentIdentity {
    fn content_identity(&self) -> CompositorContentIdentity {
        self.clone()
    }
}

pub type CompositorDamageList = CompositorDisplayList<CompositorContentIdentity>;

impl From<CompositorDisplayList> for CompositorDamageList {
    fn from(value: CompositorDisplayList) -> Self {
        Self {
            output: value.output,
            commands: value
                .commands
                .into_iter()
                .map(|command| match command {
                    CompositorDisplayCommand::Surface { surface } => {
                        CompositorDisplayCommand::Surface { surface }
                    }
                    CompositorDisplayCommand::SurfacePreview(preview) => {
                        CompositorDisplayCommand::SurfacePreview(preview)
                    }
                    CompositorDisplayCommand::Border(value) => {
                        CompositorDisplayCommand::Border(value)
                    }
                    CompositorDisplayCommand::Rect(value) => CompositorDisplayCommand::Rect(value),
                    CompositorDisplayCommand::Text(value) => CompositorDisplayCommand::Text(value),
                    CompositorDisplayCommand::IndicatorStrip(value) => {
                        CompositorDisplayCommand::IndicatorStrip(value)
                    }
                    CompositorDisplayCommand::ContentImage(value) => {
                        CompositorDisplayCommand::ContentImage(value.content_identity())
                    }
                })
                .collect(),
        }
    }
}

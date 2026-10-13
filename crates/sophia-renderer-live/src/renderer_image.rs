pub use sophia_renderer_native_egl::NativeRendererImageCustody as LiveRendererImageCustody;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct LiveRendererImageId(u64);

impl LiveRendererImageId {
    pub const INVALID: Self = Self(0);

    pub const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }

    pub const fn raw(self) -> u64 {
        self.0
    }

    pub const fn is_valid(self) -> bool {
        self.0 != 0
    }
}

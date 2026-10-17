// Shared with every role contract through the Rust desktop SDK.
pub use sophia_desktop_ids::{OutputId, TransactionId};

macro_rules! simple_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name(u64);

        impl $name {
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
    };
}

simple_id!(WmActionId);

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SurfaceId {
    index: u32,
    generation: u32,
}

impl SurfaceId {
    pub const INVALID: Self = Self {
        index: u32::MAX,
        generation: 0,
    };

    pub const fn new(index: u32, generation: u32) -> Self {
        Self { index, generation }
    }

    pub const fn index(self) -> u32 {
        self.index
    }

    pub const fn generation(self) -> u32 {
        self.generation
    }

    pub const fn is_valid(self) -> bool {
        self.index != u32::MAX && self.generation != 0
    }
}

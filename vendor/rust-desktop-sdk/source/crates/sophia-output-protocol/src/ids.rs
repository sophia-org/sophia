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

simple_id!(DisplayHeadId);
simple_id!(DisplayModeId);

//! Synthetic filesystem hierarchy implementation.

pub mod node;
pub mod tree;

pub use node::NodeKind;
pub use tree::{SyntheticTree, WindowState};

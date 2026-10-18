//! The implementation strings a rendering GL context reports for itself.
use std::sync::Arc;

/// `GL_VENDOR`, `GL_RENDERER` and `GL_VERSION` read from a current context.
/// EGL strings name only the EGL API and are never used here.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeGlIdentity {
    pub vendor: String,
    pub renderer: String,
    pub version: String,
}

impl NativeGlIdentity {
    /// An identity only when every string is non-empty.
    pub fn from_strings(vendor: String, renderer: String, version: String) -> Option<Self> {
        (!vendor.is_empty() && !renderer.is_empty() && !version.is_empty()).then_some(Self {
            vendor,
            renderer,
            version,
        })
    }
}

/// What the contexts behind one renderer have reported so far. A different
/// identity never replaces an earlier one: it records the conflict instead.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum NativeGlIdentityState {
    #[default]
    Unobserved,
    Observed(Arc<NativeGlIdentity>),
    Conflicting {
        first: Arc<NativeGlIdentity>,
        other: Arc<NativeGlIdentity>,
    },
}

impl NativeGlIdentityState {
    pub fn observe(&mut self, identity: Arc<NativeGlIdentity>) {
        match self {
            Self::Unobserved => *self = Self::Observed(identity),
            Self::Observed(first) if *first != identity => {
                *self = Self::Conflicting {
                    first: first.clone(),
                    other: identity,
                };
            }
            Self::Observed(_) | Self::Conflicting { .. } => {}
        }
    }

    /// Fold in another state. Unobserved changes nothing; a conflict stays.
    pub fn merge(&mut self, other: &Self) {
        match other {
            Self::Unobserved => {}
            Self::Observed(identity) => self.observe(identity.clone()),
            Self::Conflicting { first, other } => {
                self.observe(first.clone());
                self.observe(other.clone());
            }
        }
    }

    /// The single identity, or nothing while unobserved or conflicting.
    pub fn observed(&self) -> Option<&NativeGlIdentity> {
        match self {
            Self::Observed(identity) => Some(identity),
            Self::Unobserved | Self::Conflicting { .. } => None,
        }
    }
}

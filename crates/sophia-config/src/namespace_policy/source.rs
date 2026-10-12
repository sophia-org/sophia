//! Immutable naming input paired with the trusted policy that selected it.
use super::{NamespaceRecipeConfig, NamespaceRecipeProfile};
use crate::namespace_recipe::PinnedNamespaceRecipe;
use std::collections::BTreeMap;
use std::io;
use std::path::Path;

const X_RECIPE: &str = "namespace-recipe 1\ncd /roles\nmount x-group x11\nmount portal portal\n";
const OBSERVER_RECIPE: &str = "namespace-recipe 1\ncd /roles\nmount inspection inspection\nmount portal portal\nbind inspection status\n";

enum Source {
    Builtin {
        text: &'static str,
        fragments: BTreeMap<String, String>,
    },
    Pinned(PinnedNamespaceRecipe),
}

/// Load on the configuration/launch worker. Later edits to the configuration
/// object cannot change this snapshot; custom files are rechecked at publish.
/// This prepares naming and policy data, not service grants or containment.
pub struct LoadedNamespaceRecipe {
    policy: NamespaceRecipeConfig,
    source: Source,
}
impl LoadedNamespaceRecipe {
    pub fn load(policy: &NamespaceRecipeConfig, trusted_root: &Path) -> io::Result<Self> {
        let source = match policy.source.as_deref() {
            Some(entry) => Source::Pinned(PinnedNamespaceRecipe::load(trusted_root, entry)?),
            None => Source::Builtin {
                text: match policy.profile {
                    NamespaceRecipeProfile::SharedX | NamespaceRecipeProfile::ConfinedX { .. } => {
                        X_RECIPE
                    }
                    NamespaceRecipeProfile::Observer => OBSERVER_RECIPE,
                },
                fragments: BTreeMap::new(),
            },
        };
        Ok(Self {
            policy: policy.clone(),
            source,
        })
    }
    pub fn policy(&self) -> &NamespaceRecipeConfig {
        &self.policy
    }
    pub fn source(&self) -> &str {
        match &self.source {
            Source::Builtin { text, .. } => text,
            Source::Pinned(source) => source.source(),
        }
    }
    pub fn files(&self) -> &BTreeMap<String, String> {
        match &self.source {
            Source::Builtin { fragments, .. } => fragments,
            Source::Pinned(source) => source.files(),
        }
    }
    pub fn verify(&self) -> io::Result<()> {
        match &self.source {
            Source::Builtin { .. } => Ok(()),
            Source::Pinned(source) => source.verify(),
        }
    }
}

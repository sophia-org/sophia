//! Trusted launch policy. Application identity and metadata select policy;
//! neither executable paths nor recipe names supplied by a client grant it.
use std::collections::BTreeMap;

pub const MAX_NAMESPACE_RECIPES: usize = 64;
pub const MAX_NAMESPACE_APPLICATIONS: usize = 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NamespaceRecipeProfile {
    SharedX,
    ConfinedX { group: u32 },
    Observer,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum RecipePortalCapability {
    Clipboard,
    DragAndDrop,
    FileHandoff,
    ScreenCapture,
    ScreenRecording,
    UriOpen,
    Notification,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NamespaceRecipeConfig {
    pub name: String,
    pub profile: NamespaceRecipeProfile,
    /// Relative to the trusted core configuration root. None selects the
    /// shipped naming fragment for this profile; never an application path.
    pub source: Option<String>,
    pub request: Vec<RecipePortalCapability>,
    pub publish: Vec<RecipePortalCapability>,
    pub developer_launch: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct NamespacePolicyConfig {
    pub recipes: BTreeMap<String, NamespaceRecipeConfig>,
    /// Exact catalog identities: registered:<id> or desktop:<desktop-file-id>.
    pub applications: BTreeMap<String, String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NamespaceSelectionError {
    InvalidIdentity,
    UnapprovedRequest,
    UnknownRecipe,
    DeveloperLaunchDenied,
}
impl std::fmt::Display for NamespaceSelectionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "namespace recipe selection refused: {self:?}")
    }
}
impl std::error::Error for NamespaceSelectionError {}

impl NamespacePolicyConfig {
    /// None preserves the existing default launch policy. Metadata can only
    /// confirm the exact policy already assigned to this identity.
    pub fn select_application(
        &self,
        identity: &str,
        requested: Option<&str>,
    ) -> Result<Option<&NamespaceRecipeConfig>, NamespaceSelectionError> {
        if !valid_application_identity(identity) {
            return Err(NamespaceSelectionError::InvalidIdentity);
        }
        let selected = self.applications.get(identity);
        if requested.is_some_and(|name| selected.map(String::as_str) != Some(name)) {
            return Err(NamespaceSelectionError::UnapprovedRequest);
        }
        selected
            .map(|name| {
                self.recipes
                    .get(name)
                    .ok_or(NamespaceSelectionError::UnknownRecipe)
            })
            .transpose()
    }

    /// The caller still needs the administrative launch admission. This is
    /// an explicit policy permission, independent of any executable allowlist.
    pub fn select_developer(
        &self,
        name: &str,
    ) -> Result<&NamespaceRecipeConfig, NamespaceSelectionError> {
        let recipe = self
            .recipes
            .get(name)
            .ok_or(NamespaceSelectionError::UnknownRecipe)?;
        if !recipe.developer_launch {
            return Err(NamespaceSelectionError::DeveloperLaunchDenied);
        }
        Ok(recipe)
    }
}

pub fn valid_application_identity(identity: &str) -> bool {
    if let Some(name) = identity.strip_prefix("registered:") {
        return !name.is_empty()
            && name.len() <= 64
            && name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'));
    }
    identity.strip_prefix("desktop:").is_some_and(|name| {
        !name.is_empty()
            && name.len() <= 1024
            && name.ends_with(".desktop")
            && !name
                .chars()
                .any(|c| c.is_control() || c == '/' || c == '\\')
    })
}

use sophia_config::{
    namespace_policy::LoadedNamespaceRecipe, namespace_recipe::PinnedNamespaceRecipe,
};
use std::{collections::BTreeMap, io};

pub(super) fn validate_catalog(
    recipe: &LoadedNamespaceRecipe,
    approved: &BTreeMap<String, super::ApprovedEndpoint>,
) -> io::Result<()> {
    use sophia_config::namespace_policy::NamespaceRecipeProfile;
    use sophia_protocol::namespace_view::EndpointProtocol;
    for (name, endpoint) in approved {
        let (protocol, interface) = match (recipe.policy().profile, name.as_str()) {
            (_, "portal") => (EndpointProtocol::NineP2000L, "portal-client"),
            (NamespaceRecipeProfile::Observer, "inspection") => {
                (EndpointProtocol::NineP2000L, "inspection")
            }
            (
                NamespaceRecipeProfile::SharedX | NamespaceRecipeProfile::ConfinedX { .. },
                "x-group",
            ) => (EndpointProtocol::X11, "x11"),
            _ => {
                return Err(super::refused(
                    "service is outside the selected recipe profile",
                ));
            }
        };
        if endpoint.service.protocol != protocol || endpoint.service.interface != interface {
            return Err(super::refused(
                "selected recipe service has the wrong interface",
            ));
        }
    }
    Ok(())
}

/// Both forms use the same compiler and publication transaction. This enum
/// cannot introduce arbitrary source callbacks into the policy owner.
pub(super) enum NamingInput<'a> {
    Pinned(&'a PinnedNamespaceRecipe),
    Selected(&'a LoadedNamespaceRecipe),
}
impl NamingInput<'_> {
    pub fn source(&self) -> &str {
        match self {
            Self::Pinned(source) => source.source(),
            Self::Selected(source) => source.source(),
        }
    }
    pub fn files(&self) -> &BTreeMap<String, String> {
        match self {
            Self::Pinned(source) => source.files(),
            Self::Selected(source) => source.files(),
        }
    }
    pub fn verify(&self) -> io::Result<()> {
        match self {
            Self::Pinned(source) => source.verify(),
            Self::Selected(source) => source.verify(),
        }
    }
}

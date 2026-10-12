use super::*;
use crate::namespace_policy::*;

pub(super) fn parse(node: &KdlNode) -> Result<NamespacePolicyConfig, ConfigParseError> {
    exact_shape(node, 0, &[], true)?;
    let body = children(node)?;
    validate_root_names(body, &["recipe", "application"])?;
    let mut policy = NamespacePolicyConfig::default();
    for child in body.nodes() {
        match child.name().value() {
            "recipe" => {
                let recipe = recipe(child)?;
                if policy.recipes.len() >= MAX_NAMESPACE_RECIPES
                    || policy.recipes.insert(recipe.name.clone(), recipe).is_some()
                {
                    return schema_error("duplicate or excessive namespace recipes");
                }
            }
            "application" => {
                exact_shape(child, 1, &["recipe"], false)?;
                let identity = string_argument(child, 0, 1, 1032)?;
                let name = required_string_property(child, "recipe", 1, 64)?;
                if !valid_application_identity(identity)
                    || policy.applications.len() >= MAX_NAMESPACE_APPLICATIONS
                    || policy
                        .applications
                        .insert(identity.into(), name.into())
                        .is_some()
                {
                    return schema_error(
                        "invalid, duplicate or excessive namespace application identities",
                    );
                }
            }
            _ => unreachable!(),
        }
    }
    if policy
        .applications
        .values()
        .any(|name| !policy.recipes.contains_key(name))
    {
        return schema_error("namespace application names an unknown recipe");
    }
    Ok(policy)
}

fn recipe(node: &KdlNode) -> Result<NamespaceRecipeConfig, ConfigParseError> {
    exact_shape(
        node,
        1,
        &["profile", "group", "source", "developer-launch"],
        true,
    )?;
    let name = string_argument(node, 0, 1, 64)?.to_owned();
    if !valid_recipe_name(&name) {
        return schema_error(
            "namespace recipe name accepts 1-64 lowercase ASCII letters, digits, '-' and '_'",
        );
    }
    let group = optional_integer_property_u64(node, "group", 1, u32::MAX.into())?;
    let profile = match required_string_property(node, "profile", 1, 32)? {
        "shared-x" if group.is_none() => NamespaceRecipeProfile::SharedX,
        "observer" if group.is_none() => NamespaceRecipeProfile::Observer,
        "confined-x" if group.is_some() => NamespaceRecipeProfile::ConfinedX {
            group: group.unwrap() as u32,
        },
        _ => {
            return schema_error(
                "namespace profile requires shared-x, observer, or confined-x with a nonzero group",
            );
        }
    };
    let source = node
        .entry("source")
        .map(|_| {
            let value = required_string_property(node, "source", 1, 1024)?;
            if value.chars().any(char::is_control)
                || !value.split('/').all(|part| {
                    !part.is_empty() && part != "." && part != ".." && part.len() <= 255
                })
            {
                return schema_error(
                    "namespace source must be a normalized relative configuration path",
                );
            }
            Ok(value.to_owned())
        })
        .transpose()?;
    let mut recipe = NamespaceRecipeConfig {
        name,
        profile,
        source,
        request: Vec::new(),
        publish: Vec::new(),
        developer_launch: optional_bool_property(node, "developer-launch", false)?,
    };
    if recipe.developer_launch && profile == NamespaceRecipeProfile::SharedX {
        return schema_error("developer namespace launch requires a confined profile");
    }
    if let Some(body) = node.children() {
        validate_root_names(body, &["request", "publish"])?;
        for child in body.nodes() {
            exact_shape(child, 1, &[], false)?;
            let capability = match string_argument(child, 0, 1, 32)? {
                "clipboard" => RecipePortalCapability::Clipboard,
                "drag-and-drop" => RecipePortalCapability::DragAndDrop,
                "file-handoff" => RecipePortalCapability::FileHandoff,
                "screen-capture" => RecipePortalCapability::ScreenCapture,
                "screen-recording" => RecipePortalCapability::ScreenRecording,
                "uri-open" => RecipePortalCapability::UriOpen,
                "notification" => RecipePortalCapability::Notification,
                _ => return schema_error("unknown recipe portal capability"),
            };
            let capabilities = if child.name().value() == "request" {
                &mut recipe.request
            } else {
                &mut recipe.publish
            };
            if capabilities.contains(&capability) {
                return schema_error("duplicate recipe portal capability");
            }
            capabilities.push(capability);
        }
    }
    if profile == NamespaceRecipeProfile::Observer {
        if !recipe.publish.is_empty()
            || recipe
                .request
                .iter()
                .any(|c| *c != RecipePortalCapability::ScreenCapture)
        {
            return schema_error("observer permits only screen-capture requests");
        }
        recipe.request = vec![RecipePortalCapability::ScreenCapture];
    }
    Ok(recipe)
}

use std::collections::BTreeSet;

use kdl::{KdlDocument, KdlNode};

use crate::{
    ConfigDigest, ConfigGeneration, DesktopAuthority, DesktopAuthorityCandidate,
    DesktopProfileError,
};

pub const DESKTOP_OUTPUT_MAX_NAMED: usize = 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DesktopOutputMode {
    Preferred,
    Exact {
        width: u32,
        height: u32,
        refresh_millihz: u32,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DesktopOutputScale {
    Automatic,
    FixedMilli(u32),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DesktopOutputTransform {
    Normal,
    Rotate90,
    Rotate180,
    Rotate270,
    Flipped,
    Flipped90,
    Flipped180,
    Flipped270,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DesktopOutputVrrMode {
    Disabled,
    Automatic,
    Always,
}

/// How a mirror group's frame is placed on a head whose mode differs from the
/// scene. `wl-mirror`'s vocabulary, which operators of other mirroring tools
/// already know.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum DesktopMirrorFit {
    /// Scale until the whole frame fits, accepting black bars. Nothing is hidden,
    /// which is why it is the default.
    #[default]
    Fit,
    /// Scale until the frame covers the head, cropping the overflow.
    Cover,
    /// No scaling; centred at its own size and cropped if the head is smaller.
    Exact,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DesktopNamedOutputCandidate {
    pub connector: String,
    /// Stable operator-selected policy affinity; never inferred from enumeration.
    pub policy_key: Option<u64>,
    pub mode: Option<DesktopOutputMode>,
    pub scale: Option<DesktopOutputScale>,
    pub position: Option<(i32, i32)>,
    pub transform: Option<DesktopOutputTransform>,
    pub enabled: Option<bool>,
    pub focus_at_startup: Option<bool>,
    pub vrr: Option<DesktopOutputVrrMode>,
    /// How this group's frame lands on a head at a different mode. Meaningless
    /// without `mirror`, and `None` means the default rather than a third state.
    pub mirror_fit: Option<DesktopMirrorFit>,
    /// Connectors this output mirrors onto, making one logical output backed by
    /// many heads. Empty for an ordinary output.
    ///
    /// Named from the primary rather than from each member because policy sees one
    /// `SnapshotOutput` and no connector identity; the group has to have a single
    /// owner, and the configured output is it.
    pub mirror: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DesktopOutputCandidate {
    pub generation: ConfigGeneration,
    pub digest: ConfigDigest,
    pub inherit_sophia: bool,
    pub named: Vec<DesktopNamedOutputCandidate>,
}

impl DesktopOutputCandidate {
    /// The connector sets this configuration asks to drive as one logical output.
    ///
    /// Each group leads with its primary, because that is the output policy sees
    /// and the one the members take their state from. An output with no `mirror`
    /// contributes no group: it is its own logical output, and saying so with a
    /// single-member group would imply a shared identity it does not have.
    ///
    /// Returned as plain connector names rather than a KMS type, because
    /// configuration is the wrong layer to know what a DRM connector handle is.
    /// The session that builds the cards turns these into a grouping.
    /// The fit policy this configuration asks for.
    ///
    /// One policy per session rather than per group: a desktop with two groups
    /// wanting different placement is a shape nobody has asked for, and carrying
    /// it would mean threading a policy per logical output through the scanout for
    /// no operator benefit. The first group that states one wins; the rest default.
    pub fn mirror_fit(&self) -> Option<DesktopMirrorFit> {
        self.named
            .iter()
            .filter(|output| !output.mirror.is_empty())
            .find_map(|output| output.mirror_fit)
    }

    pub fn mirror_groups(&self) -> Vec<Vec<String>> {
        self.named
            .iter()
            .filter(|output| !output.mirror.is_empty())
            .map(|output| {
                let mut group = Vec::with_capacity(output.mirror.len() + 1);
                group.push(output.connector.clone());
                group.extend(output.mirror.iter().cloned());
                group
            })
            .collect()
    }
}

fn schema_error(message: impl Into<String>) -> DesktopProfileError {
    DesktopProfileError::Schema(format!("output candidate: {}", message.into()))
}

fn single_node(encoded: &str) -> Result<KdlNode, DesktopProfileError> {
    let document = KdlDocument::parse_v2(encoded)
        .map_err(|error| schema_error(format!("invalid staged value: {error}")))?;
    if document.nodes().len() != 1 {
        return Err(schema_error("staged value must contain exactly one node"));
    }
    Ok(document.nodes()[0].clone())
}

fn children<'a>(node: &'a KdlNode, setting: &str) -> Result<&'a KdlDocument, DesktopProfileError> {
    if node.ty().is_some() {
        return Err(schema_error(format!("{setting} has an ambiguous shape")));
    }
    node.children()
        .ok_or_else(|| schema_error(format!("{setting} requires children")))
}

fn one_bool(node: &KdlNode, setting: &str) -> Result<bool, DesktopProfileError> {
    if node.entries().len() != 1 || node.children().is_some() || node.ty().is_some() {
        return Err(schema_error(format!("{setting} requires one boolean")));
    }
    node.get(0)
        .and_then(|value| value.as_bool())
        .ok_or_else(|| schema_error(format!("{setting} requires one boolean")))
}

fn one_integer(
    node: &KdlNode,
    setting: &str,
    minimum: i128,
    maximum: i128,
) -> Result<i128, DesktopProfileError> {
    if node.entries().len() != 1 || node.children().is_some() || node.ty().is_some() {
        return Err(schema_error(format!("{setting} requires one integer")));
    }
    node.get(0)
        .and_then(|value| value.as_integer())
        .filter(|value| (minimum..=maximum).contains(value))
        .ok_or_else(|| schema_error(format!("{setting} is outside its supported range")))
}

fn one_number(node: &KdlNode, setting: &str) -> Result<f64, DesktopProfileError> {
    if node.entries().len() != 1 || node.children().is_some() || node.ty().is_some() {
        return Err(schema_error(format!("{setting} requires one number")));
    }
    node.get(0)
        .and_then(|value| {
            value.as_float().or_else(|| {
                value
                    .as_integer()
                    .and_then(|integer| i64::try_from(integer).ok())
                    .map(|integer| integer as f64)
            })
        })
        .filter(|value| value.is_finite())
        .ok_or_else(|| schema_error(format!("{setting} requires one finite number")))
}

fn one_string<'a>(node: &'a KdlNode, setting: &str) -> Result<&'a str, DesktopProfileError> {
    if node.entries().len() != 1 || node.children().is_some() || node.ty().is_some() {
        return Err(schema_error(format!("{setting} requires one string")));
    }
    node.get(0)
        .and_then(|value| value.as_string())
        .ok_or_else(|| schema_error(format!("{setting} requires one string")))
}

fn connector_name(node: &KdlNode) -> Result<String, DesktopProfileError> {
    if node.entries().len() != 1 || node.ty().is_some() {
        return Err(schema_error("named output requires one connector identity"));
    }
    let connector = node
        .get(0)
        .and_then(|value| value.as_string())
        .filter(|value| !value.is_empty() && value.len() <= 64)
        .ok_or_else(|| schema_error("named output connector identity is invalid"))?;
    if !valid_desktop_output_connector(connector) {
        return Err(schema_error(
            "named output connector contains unsupported characters",
        ));
    }
    Ok(connector.to_owned())
}

pub(crate) fn valid_desktop_output_connector(connector: &str) -> bool {
    !connector.is_empty()
        && connector.len() <= 64
        && connector
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn refresh_millihz(source: &str) -> Option<u32> {
    let (whole, fractional) = source.split_once('.').unwrap_or((source, ""));
    if whole.is_empty()
        || whole.len() > 4
        || !whole.bytes().all(|byte| byte.is_ascii_digit())
        || fractional.len() > 3
        || !fractional.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    let whole = whole.parse::<u32>().ok()?;
    let mut fractional_value = if fractional.is_empty() {
        0
    } else {
        fractional.parse::<u32>().ok()?
    };
    for _ in fractional.len()..3 {
        fractional_value = fractional_value.checked_mul(10)?;
    }
    whole.checked_mul(1_000)?.checked_add(fractional_value)
}

fn output_mode(node: &KdlNode) -> Result<DesktopOutputMode, DesktopProfileError> {
    let source = one_string(node, "output mode")?;
    if source == "preferred" {
        return Ok(DesktopOutputMode::Preferred);
    }
    let (dimensions, refresh) = source
        .split_once('@')
        .ok_or_else(|| schema_error("output mode must be preferred or WIDTHxHEIGHT@REFRESH"))?;
    let (width, height) = dimensions
        .split_once('x')
        .ok_or_else(|| schema_error("output mode must be preferred or WIDTHxHEIGHT@REFRESH"))?;
    let width = width
        .parse::<u32>()
        .ok()
        .filter(|value| (1..=16_384).contains(value));
    let height = height
        .parse::<u32>()
        .ok()
        .filter(|value| (1..=16_384).contains(value));
    let refresh_millihz =
        refresh_millihz(refresh).filter(|value| (1_000..=1_000_000).contains(value));
    match (width, height, refresh_millihz) {
        (Some(width), Some(height), Some(refresh_millihz)) => Ok(DesktopOutputMode::Exact {
            width,
            height,
            refresh_millihz,
        }),
        _ => Err(schema_error("output mode is outside its supported range")),
    }
}

fn output_scale(node: &KdlNode) -> Result<DesktopOutputScale, DesktopProfileError> {
    if node
        .get(0)
        .and_then(|value| value.as_string())
        .is_some_and(|value| value == "auto")
    {
        if node.entries().len() == 1 && node.children().is_none() && node.ty().is_none() {
            return Ok(DesktopOutputScale::Automatic);
        }
        return Err(schema_error("output scale has an ambiguous shape"));
    }
    let scale = one_number(node, "output scale")?;
    if !(0.25..=8.0).contains(&scale) {
        return Err(schema_error("output scale is outside its supported range"));
    }
    let milli = (scale * 1_000.0).round();
    if ((milli / 1_000.0) - scale).abs() > f64::EPSILON {
        return Err(schema_error("output scale supports at most three decimals"));
    }
    Ok(DesktopOutputScale::FixedMilli(milli as u32))
}

fn output_position(node: &KdlNode) -> Result<(i32, i32), DesktopProfileError> {
    if node.entries().len() != 2 || node.children().is_some() || node.ty().is_some() {
        return Err(schema_error("output position requires two integers"));
    }
    let coordinate = |index| {
        node.get(index)
            .and_then(|value| value.as_integer())
            .filter(|value| (-1_000_000..=1_000_000).contains(value))
            .and_then(|value| i32::try_from(value).ok())
            .ok_or_else(|| schema_error("output position is outside its supported range"))
    };
    Ok((coordinate(0)?, coordinate(1)?))
}

fn output_transform(node: &KdlNode) -> Result<DesktopOutputTransform, DesktopProfileError> {
    Ok(match one_string(node, "output transform")? {
        "normal" => DesktopOutputTransform::Normal,
        "90" => DesktopOutputTransform::Rotate90,
        "180" => DesktopOutputTransform::Rotate180,
        "270" => DesktopOutputTransform::Rotate270,
        "flipped" => DesktopOutputTransform::Flipped,
        "flipped-90" => DesktopOutputTransform::Flipped90,
        "flipped-180" => DesktopOutputTransform::Flipped180,
        "flipped-270" => DesktopOutputTransform::Flipped270,
        _ => return Err(schema_error("unsupported output transform")),
    })
}

/// Parses `mirror-fit "fit"` into how a group's frame lands on a head.
///
/// Named rather than inferred, so an operator who would rather crop than see
/// black bars can say so. Refusing an unknown name matters more here than most
/// config: silently defaulting would put the wrong thing on a screen and give
/// nothing to search for.
fn output_mirror_fit(node: &KdlNode) -> Result<DesktopMirrorFit, DesktopProfileError> {
    match one_string(node, "output mirror-fit")? {
        "fit" => Ok(DesktopMirrorFit::Fit),
        "cover" => Ok(DesktopMirrorFit::Cover),
        "exact" => Ok(DesktopMirrorFit::Exact),
        _ => Err(schema_error(
            "output mirror-fit must be fit, cover, or exact",
        )),
    }
}

/// Parses `mirror "DP-2" "DP-3"` into the connectors a primary drives.
///
/// Every rule here is about the request being expressible at all, so it is checked
/// before the topology is consulted: a group with no members, a connector that
/// mirrors itself, or the same connector twice are configuration mistakes whatever
/// hardware is attached.
fn output_mirror(node: &KdlNode, primary: &str) -> Result<Vec<String>, DesktopProfileError> {
    if node.entries().is_empty() || node.ty().is_some() {
        return Err(schema_error(
            "output mirror requires at least one connector",
        ));
    }
    if node.entries().len() >= DESKTOP_OUTPUT_MAX_NAMED {
        return Err(schema_error("output mirror names too many connectors"));
    }
    let mut mirrored = Vec::with_capacity(node.entries().len());
    for entry in node.entries() {
        let connector = entry
            .value()
            .as_string()
            .filter(|value| !value.is_empty() && value.len() <= 64)
            .ok_or_else(|| schema_error("output mirror connector identity is invalid"))?;
        if !valid_desktop_output_connector(connector) {
            return Err(schema_error(
                "output mirror connector contains unsupported characters",
            ));
        }
        if connector == primary {
            return Err(schema_error("output mirror names its own connector"));
        }
        if mirrored.iter().any(|seen| seen == connector) {
            return Err(schema_error("output mirror repeats a connector"));
        }
        mirrored.push(connector.to_owned());
    }
    Ok(mirrored)
}

fn named_output(node: &KdlNode) -> Result<DesktopNamedOutputCandidate, DesktopProfileError> {
    let connector = connector_name(node)?;
    let mut result = DesktopNamedOutputCandidate {
        connector,
        policy_key: None,
        mode: None,
        scale: None,
        position: None,
        transform: None,
        enabled: None,
        focus_at_startup: None,
        mirror_fit: None,
        vrr: None,
        mirror: Vec::new(),
    };
    let children = children(node, "named output")?;
    if children.nodes().is_empty() {
        return Err(schema_error("named output requires at least one setting"));
    }
    for child in children.nodes() {
        match child.name().value() {
            "policy-key" if result.policy_key.is_none() => {
                result.policy_key =
                    Some(one_integer(child, "output policy-key", 1, i128::from(i64::MAX))? as u64);
            }
            "mode" if result.mode.is_none() => result.mode = Some(output_mode(child)?),
            "scale" if result.scale.is_none() => result.scale = Some(output_scale(child)?),
            "position" if result.position.is_none() => {
                result.position = Some(output_position(child)?)
            }
            "transform" if result.transform.is_none() => {
                result.transform = Some(output_transform(child)?)
            }
            "enabled" if result.enabled.is_none() => {
                result.enabled = Some(one_bool(child, "output enabled")?)
            }
            "focus-at-startup" if result.focus_at_startup.is_none() => {
                result.focus_at_startup = Some(one_bool(child, "output focus-at-startup")?)
            }
            "vrr" if result.vrr.is_none() => {
                result.vrr = Some(match one_integer(child, "output vrr", 0, 2)? {
                    0 => DesktopOutputVrrMode::Disabled,
                    1 => DesktopOutputVrrMode::Automatic,
                    2 => DesktopOutputVrrMode::Always,
                    _ => unreachable!("bounded VRR mode"),
                })
            }
            "mirror" if result.mirror.is_empty() => {
                result.mirror = output_mirror(child, &result.connector)?;
            }
            "mirror-fit" if result.mirror_fit.is_none() => {
                result.mirror_fit = Some(output_mirror_fit(child)?);
            }
            "mode" | "scale" | "position" | "transform" | "enabled" | "focus-at-startup"
            | "vrr" | "mirror" | "mirror-fit" | "policy-key" => {
                return Err(schema_error("duplicate named output setting"));
            }
            _ => return Err(schema_error("unsupported named output setting")),
        }
    }
    Ok(result)
}

pub fn prepare_desktop_output_candidate(
    candidate: &DesktopAuthorityCandidate,
) -> Result<DesktopOutputCandidate, DesktopProfileError> {
    if candidate.authority != DesktopAuthority::Output {
        return Err(schema_error("candidate crossed its authority boundary"));
    }
    let mut prepared = DesktopOutputCandidate {
        generation: candidate.generation,
        digest: candidate.digest,
        inherit_sophia: true,
        named: Vec::new(),
    };
    let mut inheritance_seen = false;
    let mut connectors = BTreeSet::new();
    let mut focused_connector = None;
    let mut policy_keys = BTreeSet::new();
    for value in &candidate.values {
        let node = single_node(&value.encoded)?;
        match node.name().value() {
            "inherit-sophia" if !inheritance_seen => {
                prepared.inherit_sophia = one_bool(&node, "inherit-sophia")?;
                inheritance_seen = true;
            }
            "named" => {
                if prepared.named.len() == DESKTOP_OUTPUT_MAX_NAMED {
                    return Err(schema_error("too many named outputs"));
                }
                let output = named_output(&node)?;
                if !connectors.insert(output.connector.clone()) {
                    return Err(schema_error("duplicate named output connector"));
                }
                if output.focus_at_startup == Some(true)
                    && focused_connector
                        .replace(output.connector.clone())
                        .is_some()
                {
                    return Err(schema_error("more than one output requests startup focus"));
                }
                if output
                    .policy_key
                    .is_some_and(|key| !policy_keys.insert(key))
                {
                    return Err(schema_error("duplicate output policy-key"));
                }
                prepared.named.push(output);
            }
            "inherit-sophia" => return Err(schema_error("duplicate output setting")),
            _ => return Err(schema_error("candidate contains a non-output setting")),
        }
    }
    if !prepared.inherit_sophia && prepared.named.is_empty() {
        return Err(schema_error(
            "non-inheriting output candidate requires a named output",
        ));
    }
    Ok(prepared)
}

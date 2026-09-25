//! Validate the complete published shell revision independently of codecs.
use super::{integer_property, string_arg, string_property};
use kdl::KdlDocument;
use std::collections::BTreeMap;

pub(super) fn validate(text: &str) -> Result<(), String> {
    let document: KdlDocument = text.parse().map_err(|error: kdl::KdlError| {
        let details = error
            .diagnostics
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("; ");
        format!("parse shell schema: {details}")
    })?;
    let protocol = document
        .get("protocol")
        .ok_or("shell schema must contain one protocol node")?;
    if string_arg(protocol, 0)? != "sophia_shell_v1"
        || integer_property(protocol, "frame-version")? != 1
        || integer_property(protocol, "interface-major")? != 1
        || integer_property(protocol, "interface-revision")? != 9
        || integer_property(protocol, "max-descriptors")? != 16
        || integer_property(protocol, "max-label-bytes")? != 128
        || integer_property(protocol, "max-pending-activations")? != 16
    {
        return Err("shell schema envelope or revision constants drifted".into());
    }
    let children = protocol
        .children()
        .ok_or("shell protocol node must have children")?;
    let expected = BTreeMap::from([
        ("OverviewBegin", (123, "session-to-shell", "required")),
        ("OverviewWorkspace", (124, "session-to-shell", "required")),
        ("OverviewEnd", (125, "session-to-shell", "required")),
        ("OverviewRequest", (126, "session-to-shell", "required")),
        ("OverviewCandidate", (127, "shell-to-session", "required")),
        ("OverviewOutcome", (128, "session-to-shell", "required")),
        ("ClientHello", (96, "shell-to-session", "zero")),
        ("ServerWelcome", (97, "session-to-shell", "zero")),
        ("DescriptorSnapshot", (98, "session-to-shell", "required")),
        ("Candidate", (99, "shell-to-session", "required")),
        ("CandidateOutcome", (100, "session-to-shell", "required")),
        ("Activation", (101, "session-to-shell", "required")),
        ("ActivationAck", (102, "shell-to-session", "required")),
        ("TabsBegin", (103, "session-to-shell", "required")),
        ("TabsGroup", (104, "session-to-shell", "required")),
        ("TabsEntry", (105, "session-to-shell", "required")),
        ("TabsEnd", (106, "session-to-shell", "required")),
        ("TabsCandidate", (107, "shell-to-session", "required")),
        ("ShortcutsBegin", (108, "session-to-shell", "required")),
        ("ShortcutsEntry", (109, "session-to-shell", "required")),
        ("ShortcutsEnd", (110, "session-to-shell", "required")),
        ("ReferenceRequest", (111, "session-to-shell", "required")),
        ("ReferenceCandidate", (112, "shell-to-session", "required")),
        ("ReferenceOutcome", (113, "session-to-shell", "required")),
        ("ContentAdmissionRefused", (160, "session-to-shell", "zero")),
        ("ContentLimits", (161, "session-to-shell", "zero")),
        ("ContentOutputFacts", (162, "session-to-shell", "required")),
        (
            "ContentAllocationRequest",
            (163, "shell-to-session", "required"),
        ),
        (
            "ContentAllocationResult",
            (164, "session-to-shell", "required"),
        ),
        (
            "ContentResourceBegin",
            (165, "shell-to-session", "required"),
        ),
        (
            "ContentResourceStatus",
            (166, "session-to-shell", "required"),
        ),
        (
            "ContentResourceChunk",
            (167, "shell-to-session", "required"),
        ),
        ("ContentResourceEnd", (168, "shell-to-session", "required")),
        (
            "ContentResourceCancel",
            (169, "shell-to-session", "required"),
        ),
        (
            "ContentResourceRetire",
            (170, "shell-to-session", "required"),
        ),
        (
            "ContentResourceReleased",
            (171, "session-to-shell", "required"),
        ),
        (
            "ContentCandidateBegin",
            (172, "shell-to-session", "required"),
        ),
        (
            "ContentCandidateChunk",
            (173, "shell-to-session", "required"),
        ),
        ("ContentCandidateEnd", (174, "shell-to-session", "required")),
        (
            "ContentCandidateOutcome",
            (175, "session-to-shell", "required"),
        ),
        ("ContentFrameDemand", (176, "shell-to-session", "required")),
        ("ContentFramePermit", (177, "session-to-shell", "required")),
        (
            "ContentFrameDemandCancel",
            (178, "shell-to-session", "required"),
        ),
        ("ContentAction", (179, "session-to-shell", "required")),
        ("ContentActionAck", (180, "shell-to-session", "required")),
        ("IndicatorsBegin", (181, "session-to-shell", "required")),
        (
            "IndicatorsOutputStatus",
            (182, "session-to-shell", "required"),
        ),
        ("IndicatorsEntry", (183, "session-to-shell", "required")),
        ("IndicatorsEnd", (184, "session-to-shell", "required")),
        ("IndicatorActivate", (185, "shell-to-session", "required")),
        (
            "IndicatorActivateOutcome",
            (186, "session-to-shell", "required"),
        ),
        ("ApplicationsBegin", (114, "session-to-shell", "required")),
        ("ApplicationsEntry", (115, "session-to-shell", "required")),
        ("ApplicationsEnd", (116, "session-to-shell", "required")),
        ("LauncherRequest", (117, "session-to-shell", "required")),
        ("LauncherCandidate", (118, "shell-to-session", "required")),
        ("LauncherOutcome", (119, "session-to-shell", "required")),
        ("LauncherActivation", (120, "session-to-shell", "required")),
        (
            "LauncherActivationAck",
            (121, "shell-to-session", "required"),
        ),
        ("LaunchOutcome", (122, "session-to-shell", "required")),
        (
            "NativeLauncherOpening",
            (187, "session-to-shell", "required"),
        ),
        (
            "NativeLauncherAllocationRequest",
            (188, "shell-to-session", "required"),
        ),
        (
            "NativeLauncherCandidateBegin",
            (189, "shell-to-session", "required"),
        ),
        (
            "NativeLauncherCandidateChunk",
            (190, "shell-to-session", "required"),
        ),
        ("NativeLauncherFocus", (191, "session-to-shell", "required")),
        (
            "NativeLauncherFocusRevoked",
            (192, "session-to-shell", "required"),
        ),
        ("NativeLauncherInput", (193, "session-to-shell", "required")),
        (
            "NativeLauncherInputAck",
            (194, "shell-to-session", "required"),
        ),
        (
            "NativeLauncherActivate",
            (195, "shell-to-session", "required"),
        ),
        (
            "NativeLauncherActivationOutcome",
            (196, "session-to-shell", "required"),
        ),
        (
            "NativeLauncherClosed",
            (197, "session-to-shell", "required"),
        ),
        (
            "CatalogCandidateBegin",
            (198, "shell-to-session", "required"),
        ),
        (
            "CatalogCandidateChunk",
            (199, "shell-to-session", "required"),
        ),
        ("CatalogActivate", (200, "shell-to-session", "required")),
        (
            "CatalogActivationOutcome",
            (201, "session-to-shell", "required"),
        ),
        ("CatalogIdentity", (202, "session-to-shell", "required")),
    ]);
    let expected_count = expected.len();
    let mut actual = BTreeMap::new();
    for message in children
        .nodes()
        .iter()
        .filter(|node| node.name().value() == "message")
    {
        actual.insert(
            string_arg(message, 0)?,
            (
                integer_property(message, "kind")?,
                string_property(message, "direction")?,
                string_property(message, "transaction")?,
            ),
        );
    }
    for (name, (kind, direction, transaction)) in expected {
        let Some(actual) = actual.get(name) else {
            return Err(format!("shell schema omits message `{name}`"));
        };
        if actual.0 != kind || actual.1 != direction || actual.2 != transaction {
            return Err(format!("shell schema message `{name}` drifted"));
        }
    }
    if actual.len() != expected_count {
        return Err("shell schema revision-9 message set drifted".into());
    }
    Ok(())
}

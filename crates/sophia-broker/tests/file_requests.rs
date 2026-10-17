use sophia_broker::{MetadataBroker, MetadataBrokerCommand, MetadataBrokerRejection};
use sophia_protocol::{
    AttentionState, MetadataDisclosure, NamespaceProfile, SurfaceId,
    broker_files::BrokerFileRequest as Request,
};

#[test]
fn disclosure_lowering_preserves_both_effects_and_clears_retained_label() {
    let mut broker = MetadataBroker::new();
    let surface = SurfaceId::new(1, 1);
    let admitted = broker
        .file_request(
            Request::SurfaceAdmitted {
                surface,
                profile: NamespaceProfile::Confined,
            },
            true,
        )
        .unwrap();
    assert!(
        matches!(admitted.as_slice(), [MetadataBrokerCommand::PublishRule(rule)]
        if rule.disclosure == MetadataDisclosure::ClassOnly)
    );
    broker
        .file_request(
            Request::SetDisclosure {
                surface,
                disclosure: MetadataDisclosure::Full,
            },
            false,
        )
        .unwrap();
    broker
        .file_request(
            Request::CandidateReduced {
                surface,
                disclosure: MetadataDisclosure::Full,
                label: Some("private title"),
                redacted: false,
                generation: 1,
            },
            false,
        )
        .unwrap();
    let lowered = broker
        .file_request(
            Request::SetDisclosure {
                surface,
                disclosure: MetadataDisclosure::ClassOnly,
            },
            false,
        )
        .unwrap();
    assert!(
        matches!(lowered.as_slice(), [MetadataBrokerCommand::PublishRule(rule),
        MetadataBrokerCommand::EmitDescriptor { descriptor, action }]
        if rule.disclosure == MetadataDisclosure::ClassOnly && descriptor.label.is_none()
        && !descriptor.label_redacted && action.target_generation == descriptor.generation)
    );
    let attention = broker
        .file_request(
            Request::AttentionChanged {
                surface,
                attention: AttentionState::Critical,
            },
            false,
        )
        .unwrap();
    assert!(
        matches!(attention.as_slice(), [MetadataBrokerCommand::EmitDescriptor { descriptor, .. }]
        if descriptor.label.is_none() && descriptor.attention == AttentionState::Critical)
    );
    assert!(
        matches!(broker.file_request(Request::SurfaceRemoved { surface }, false).unwrap().as_slice(),
        [MetadataBrokerCommand::RetireSurface { surface: retired }] if *retired == surface)
    );
    assert_eq!(
        broker.file_request(Request::SurfaceRemoved { surface }, false),
        Err(MetadataBrokerRejection::UnknownSurface)
    );
}

#[test]
fn default_policy_does_not_hide_invalid_admission() {
    let mut broker = MetadataBroker::new();
    assert_eq!(
        broker.file_request(
            Request::SurfaceAdmitted {
                surface: SurfaceId::new(0, 0),
                profile: NamespaceProfile::ClassicShared,
            },
            true
        ),
        Err(MetadataBrokerRejection::UnknownSurface)
    );
    assert!(broker.is_empty());
}

#[test]
fn readmission_with_default_policy_keeps_the_descriptor_replacement() {
    let mut broker = MetadataBroker::new();
    let surface = SurfaceId::new(2, 1);
    let admission = Request::SurfaceAdmitted {
        surface,
        profile: NamespaceProfile::ClassicShared,
    };
    broker.file_request(admission, false).unwrap();
    broker
        .file_request(
            Request::SetDisclosure {
                surface,
                disclosure: MetadataDisclosure::Full,
            },
            false,
        )
        .unwrap();
    broker
        .file_request(
            Request::CandidateReduced {
                surface,
                disclosure: MetadataDisclosure::Full,
                label: Some("private"),
                redacted: false,
                generation: 7,
            },
            false,
        )
        .unwrap();
    let commands = broker.file_request(admission, true).unwrap();
    assert!(
        matches!(commands.as_slice(), [MetadataBrokerCommand::PublishRule(rule),
        MetadataBrokerCommand::EmitDescriptor { descriptor, action }]
        if rule.disclosure == MetadataDisclosure::ClassOnly && descriptor.label.is_none()
        && descriptor.generation == 7 && action.target_generation == 7)
    );
}

use sophia_engine::ChromeDescriptorTable;
use sophia_protocol::{
    AttentionState, BrokerToplevelActionGrant, IconTokenId, MetadataDisclosure,
    MetadataDisclosureRule, SanitizedChromeMetadata, SurfaceId, TrustLevel,
    broker_files::{BrokerFileRequest, decode_broker_file_response},
};
use sophia_runtime::broker_files::BrokerFileEffects;
use sophia_session::broker_file_owner::{BrokerCompletion, BrokerFileOwner};
use sophia_session::broker_responses::*;

fn put(bytes: &mut [u8], at: usize, value: u64, size: usize) {
    bytes[at..at + size].copy_from_slice(&value.to_le_bytes()[..size]);
}
fn pair() -> [u8; 448] {
    let mut b = [0; 448];
    for (at, value, size) in [
        (0, 448, 4),
        (4, 1, 2),
        (6, 257, 2),
        (8, 7, 8),
        (16, 2, 8),
        (32, 9, 8),
        (40, 5, 2),
        (42, 2, 2),
    ] {
        put(&mut b, at, value, size);
    }
    for (offset, kind) in [(48, 1), (248, 2)] {
        put(&mut b, offset, kind, 2);
        put(&mut b, offset + 8, 3, 4);
        put(&mut b, offset + 12, 4, 4);
        put(&mut b, offset + 18, 2, 2);
        b[offset + 22] = 1;
        put(&mut b, offset + 24, 11, 8);
        put(&mut b, offset + 32, 8, 8);
    }
    b[48 + 16] = 2;
    b[248 + 20] = 1;
    put(&mut b, 248 + 40, 12, 8);
    put(&mut b, 248 + 48, 1, 8);
    put(&mut b, 248 + 56, 8, 8);
    b
}
fn surface() -> SurfaceId {
    SurfaceId::new(3, 4)
}
fn request() -> BrokerFileRequest<'static> {
    BrokerFileRequest::SetDisclosure {
        surface: surface(),
        disclosure: MetadataDisclosure::ClassOnly,
    }
}
fn table() -> ChromeDescriptorTable {
    let mut table = ChromeDescriptorTable::default();
    table.apply_metadata(SanitizedChromeMetadata {
        surface: surface(),
        label: Some("Private title".into()),
        label_redacted: false,
        icon: Some(IconTokenId::from_raw(11)),
        trust_level: TrustLevel::Trusted,
        attention: AttentionState::None,
        generation: 8,
    });
    table
}
fn context(table: &ChromeDescriptorTable) -> BrokerResponseContext<'_> {
    BrokerResponseContext {
        epoch: 7,
        request_id: 9,
        request: request(),
        admitted: true,
        retired: false,
        rule: Some(MetadataDisclosureRule {
            surface: surface(),
            disclosure: MetadataDisclosure::Full,
            icon: Some(IconTokenId::from_raw(11)),
            trust_level: TrustLevel::Trusted,
            generation: 0,
        }),
        grant: Some(BrokerToplevelActionGrant {
            token: 12,
            revocation_epoch: 1,
            target_generation: 8,
        }),
        descriptors: table,
    }
}

#[test]
fn lowering_prepares_both_effects_without_mutation() {
    let table = table();
    let bytes = pair();
    let set = decode_broker_file_response(&bytes, 9, request()).unwrap();
    let prepared = context(&table).prepare(&set).unwrap();
    assert!(matches!(prepared, PreparedBrokerResponse::Effects {
        rule:Some(rule), descriptor:Some((descriptor,action)), retire:None,
    } if rule.disclosure == MetadataDisclosure::ClassOnly && descriptor.label.is_none()
        && action.token == 12 && action.target_generation == 8));
    assert_eq!(
        table.get(surface()).unwrap().label.as_ref().unwrap().text,
        "Private title"
    );
    // Readmission uses the same compound response when its default lowers policy.
    let mut bytes = bytes;
    bytes[40] = 1;
    let mut context = context(&table);
    context.request = BrokerFileRequest::SurfaceAdmitted {
        surface: surface(),
        profile: sophia_protocol::NamespaceProfile::ClassicShared,
    };
    let set = decode_broker_file_response(&bytes, 9, context.request).unwrap();
    assert!(matches!(
        context.prepare(&set),
        Ok(PreparedBrokerResponse::Effects {
            descriptor: Some(_),
            ..
        })
    ));
}

#[test]
fn second_row_owner_failures_never_prepare_the_first_rule() {
    let table = table();
    for (at, value, expected) in [
        (248 + 40, 13, BrokerResponseFailure::Grant),
        (248 + 48, 2, BrokerResponseFailure::Grant),
        (248 + 24, 14, BrokerResponseFailure::Rule),
    ] {
        let mut bytes = pair();
        put(&mut bytes, at, value, 8);
        let set = decode_broker_file_response(&bytes, 9, request()).unwrap();
        assert_eq!(context(&table).prepare(&set), Err(expected));
        assert_eq!(
            table.get(surface()).unwrap().label.as_ref().unwrap().text,
            "Private title"
        );
    }
    let mut bytes = pair();
    put(&mut bytes, 248 + 32, 7, 8);
    put(&mut bytes, 248 + 56, 7, 8);
    let set = decode_broker_file_response(&bytes, 9, request()).unwrap();
    assert_eq!(
        context(&table).prepare(&set),
        Err(BrokerResponseFailure::Generation)
    );
}

#[test]
fn missing_replacement_and_changed_routes_are_rejected() {
    let table = table();
    let mut bytes = pair();
    bytes[42] = 1;
    bytes[248..].fill(0);
    let set = decode_broker_file_response(&bytes, 9, request()).unwrap();
    assert_eq!(
        context(&table).prepare(&set),
        Err(BrokerResponseFailure::Rule)
    );
    let bytes = pair();
    let set = decode_broker_file_response(&bytes, 9, request()).unwrap();
    let mut context = context(&table);
    context.retired = true;
    assert_eq!(context.prepare(&set), Err(BrokerResponseFailure::Route));
    context.retired = false;
    context.admitted = false;
    assert_eq!(context.prepare(&set), Err(BrokerResponseFailure::Route));
    context.admitted = true;
    context.request_id = 10;
    assert_eq!(
        context.prepare(&set),
        Err(BrokerResponseFailure::Correlation)
    );
}

#[test]
fn candidate_attention_and_retirement_prepare_native_effects() {
    let table = table();
    let mut bytes = pair();
    bytes.copy_within(248..448, 48);
    bytes[248..].fill(0);
    bytes[42] = 1;
    bytes[40] = 2;
    let mut context = context(&table);
    context.request = BrokerFileRequest::CandidateReduced {
        surface: surface(),
        disclosure: MetadataDisclosure::Full,
        label: None,
        redacted: false,
        generation: 8,
    };
    let set = decode_broker_file_response(&bytes, 9, context.request).unwrap();
    assert!(matches!(
        context.prepare(&set),
        Ok(PreparedBrokerResponse::Effects {
            rule: None,
            descriptor: Some(_),
            retire: None,
        })
    ));
    // The wire accepts these labels; Engine's presentation boundary does not.
    for label in ["", "bad\nlabel"] {
        let mut bad = bytes;
        bad[48 + 64] = 1;
        put(&mut bad, 48 + 66, label.len() as u64, 2);
        bad[48 + 72..48 + 72 + label.len()].copy_from_slice(label.as_bytes());
        let set = decode_broker_file_response(&bad, 9, context.request).unwrap();
        assert_eq!(
            context.prepare(&set),
            Err(BrokerResponseFailure::Descriptor)
        );
    }
    bytes[40] = 3;
    bytes[48 + 20] = 3;
    context.request = BrokerFileRequest::AttentionChanged {
        surface: surface(),
        attention: AttentionState::Critical,
    };
    let set = decode_broker_file_response(&bytes, 9, context.request).unwrap();
    assert!(
        matches!(context.prepare(&set),Ok(PreparedBrokerResponse::Effects {
        descriptor:Some((descriptor,_)), ..
    }) if descriptor.attention == AttentionState::Critical)
    );
    bytes[40] = 4;
    bytes[48..].fill(0);
    bytes[48] = 3;
    put(&mut bytes, 56, 3, 4);
    put(&mut bytes, 60, 4, 4);
    context.request = BrokerFileRequest::SurfaceRemoved { surface: surface() };
    let set = decode_broker_file_response(&bytes, 9, context.request).unwrap();
    assert_eq!(
        context.prepare(&set),
        Ok(PreparedBrokerResponse::Effects {
            rule: None,
            descriptor: None,
            retire: Some(surface()),
        })
    );
    assert_eq!(
        table.get(surface()).unwrap().label.as_ref().unwrap().text,
        "Private title"
    );
}

#[test]
fn rejection_and_no_change_remain_explicit_outcomes() {
    let table = table();
    let mut bytes = pair();
    bytes[48..].fill(0);
    bytes[42] = 1;
    bytes[40] = 3;
    let mut context = context(&table);
    context.request = BrokerFileRequest::AttentionChanged {
        surface: surface(),
        attention: AttentionState::None,
    };
    bytes[48] = 5;
    let set = decode_broker_file_response(&bytes, 9, context.request).unwrap();
    assert_eq!(context.prepare(&set), Ok(PreparedBrokerResponse::NoChange));
    bytes[48] = 4;
    bytes[50] = 1;
    let set = decode_broker_file_response(&bytes, 9, context.request).unwrap();
    assert_eq!(
        context.prepare(&set),
        Ok(PreparedBrokerResponse::Rejected(1))
    );
}

fn owner() -> BrokerFileOwner<u64> {
    let mut owner = BrokerFileOwner::new(7).unwrap();
    let admission = BrokerFileRequest::SurfaceAdmitted {
        surface: surface(),
        profile: sophia_protocol::NamespaceProfile::ClassicShared,
    };
    let mut bytes = pair();
    bytes[32] = 1;
    bytes[40] = 1;
    bytes[42] = 1;
    bytes[48 + 16] = 3;
    put(&mut bytes, 48 + 32, 0, 8);
    bytes[248..].fill(0);
    owner.begin(1, admission, 42).unwrap();
    let set = decode_broker_file_response(&bytes, 1, admission).unwrap();
    let prepared = owner.prepare(&set).unwrap();
    assert!(owner.route(surface()).is_none());
    owner.commit(prepared);
    assert_eq!(owner.route(surface()), Some(42));
    assert!(owner.take_completion().is_some());
    let candidate = BrokerFileRequest::CandidateReduced {
        surface: surface(),
        disclosure: MetadataDisclosure::Full,
        label: Some("Private title"),
        redacted: false,
        generation: 8,
    };
    let mut bytes = pair();
    bytes.copy_within(248..448, 48);
    bytes[248..].fill(0);
    bytes[32] = 2;
    bytes[40] = 2;
    bytes[42] = 1;
    bytes[48 + 64] = 1;
    put(&mut bytes, 48 + 66, 13, 2);
    bytes[48 + 72..48 + 85].copy_from_slice(b"Private title");
    owner.begin(2, candidate, 42).unwrap();
    let set = decode_broker_file_response(&bytes, 2, candidate).unwrap();
    let prepared = owner.prepare(&set).unwrap();
    owner.commit(prepared);
    owner.take_completion().unwrap();
    owner
}

#[test]
fn custody_commits_owner_effects_once_and_reserves_completion() {
    use sophia_9p::Errno;
    use sophia_protocol::broker_files::BrokerFileLimits;
    use sophia_runtime::broker_files::BrokerFileCustody;
    use std::{cell::RefCell, time::Instant};
    let now = Instant::now();
    let mut custody = BrokerFileCustody::new_after(7, BrokerFileLimits::default(), 2).unwrap();
    let mut hello = [0; 40];
    hello[0] = 40;
    hello[4] = 1;
    hello[7] = 1;
    hello[8] = 7;
    hello[16] = 1;
    hello[32] = 2;
    hello[34] = 2;
    let mut control = [0; 24];
    control[0] = 7;
    control[8] = 1;
    control[16] = 40;
    custody.negotiate(&control, &hello, now).unwrap();
    let mut ack = [0; 16];
    ack[0] = 7;
    ack[8] = 2;
    custody.acknowledge(&ack, now).unwrap();
    let owner = RefCell::new(owner());
    owner.borrow_mut().begin(3, request(), 42).unwrap();
    custody.publish(3, request(), now).unwrap();
    let mut bytes = pair();
    bytes[32] = 3;
    control[8] = 2;
    put(&mut control, 16, 448, 4);
    for _ in 0..2 {
        custody
            .respond(
                &control,
                &bytes,
                now,
                |set| owner.borrow_mut().prepare(set),
                |prepared| owner.borrow_mut().commit(prepared),
            )
            .unwrap();
    }
    let mut owner = owner.into_inner();
    assert_eq!(
        owner.rule(surface()).unwrap().disclosure,
        MetadataDisclosure::ClassOnly
    );
    assert!(owner.descriptors().get(surface()).unwrap().label.is_none());
    assert_eq!(owner.grants()[&surface()].token, 12);
    assert_eq!(owner.begin(4, request(), 42), Err(Errno::EAGAIN));
    assert!(matches!(
        owner.take_completion(),
        Some(BrokerCompletion::Applied {
            request: 3,
            rule: Some(_),
            retired: None
        })
    ));
    assert!(owner.take_completion().is_none());
    owner.begin(4, request(), 42).unwrap();
    owner.abort_unpublished();
    assert_eq!(owner.begin(4, request(), 43), Err(Errno::EACCES));
}

#[test]
fn owner_failure_latches_without_mutating_state_or_publishing_completion() {
    let mut owner = owner();
    let mut bytes = pair();
    bytes[32] = 3;
    put(&mut bytes, 248 + 40, 13, 8);
    owner.begin(3, request(), 42).unwrap();
    let set = decode_broker_file_response(&bytes, 3, request()).unwrap();
    assert!(owner.prepare(&set).is_err());
    assert_eq!(owner.failure(), Some(BrokerResponseFailure::Grant));
    assert!(owner.take_completion().is_none());
    assert_eq!(
        owner.rule(surface()).unwrap().disclosure,
        MetadataDisclosure::Full
    );
    assert_eq!(
        owner
            .descriptors()
            .get(surface())
            .unwrap()
            .label
            .as_ref()
            .unwrap()
            .text,
        "Private title"
    );
    assert_eq!(owner.grants()[&surface()].token, 12);
    owner.abort_unpublished();
    assert_eq!(owner.begin(4, request(), 42), Err(sophia_9p::Errno::EIO));
}

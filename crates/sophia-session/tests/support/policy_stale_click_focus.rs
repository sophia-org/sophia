use super::*;

fn layer(surface: SurfaceId) -> LayerSnapshot {
    let geometry = Rect {
        x: 0,
        y: 0,
        width: 100,
        height: 100,
    };
    LayerSnapshot {
        surface,
        authority_local_id: None,
        namespace: None,
        stack_rank: 0,
        geometry,
        source_size: Size {
            width: 100,
            height: 100,
        },
        source: BufferSource::None,
        damage: Region::empty(),
        opacity: 1.0,
        crop: None,
        transform: Transform::IDENTITY,
        generation: 1,
        resize_sync: ResizeSyncCapability::ImplicitOnly,
        input_region: None,
        translation: None,
        output: None,
    }
}

#[test]
fn a_click_queued_before_an_empty_layout_commit_is_discarded_without_ending_the_session() {
    let mut fixture = ReloadFixture::new();
    let output = sophia_engine::HeadlessOutput::deterministic();
    let surface = SurfaceId::new(91, 1);
    let mut layout = PersistentLiveLayout::default();
    layout.layers.insert(surface, layer(surface));
    let mut inputs = PhysicalPolicyInputQueue::default();
    inputs.synchronize(Some(1));
    assert!(inputs.push(PhysicalPolicyInput::ClickFocus(surface), false));

    // Input can still name the previous presentation while a workspace
    // projection has already committed an empty current layout.
    let transaction = TransactionId::from_raw(24);
    layout.commit_proposal(LiveWmProposal {
        transaction,
        layers: vec![],
        requested_sizes: BTreeMap::new(),
        presentation_states: BTreeMap::new(),
        configure_deliveries: 0,
        focus: None,
        timeout: Duration::from_secs(1),
        update: WmTransactionUpdate {
            commit: TransactionCommit {
                transaction,
                outcome: TransactionOutcome::Committed,
                applied_surfaces: vec![],
            },
        },
        moved_surfaces: 0,
        source: Some(LiveWmProposalSource::Relayout),
        policy_settlement: None,
    });
    assert!(layout.layers.is_empty());
    let Some(PhysicalPolicyInput::ClickFocus(target)) = inputs.next(false) else {
        panic!("the ordered queue must preserve the original click identity");
    };
    assert_eq!(
        fixture
            .wm
            .enqueue_focus(target, &layout, output)
            .expect("a stale click is not a session failure"),
        LiveWmRequestAdmission::Duplicate,
    );
    let public = fixture.wm.public.as_ref().unwrap();
    assert!(public.queue.is_empty());
    assert_eq!(public.active_output, output.id);
    assert!(public.in_flight_request.is_none());
    assert!(inputs.is_empty());
}

#[test]
fn a_stale_click_does_not_retarget_a_reused_surface_index() {
    let mut fixture = ReloadFixture::new();
    let output = sophia_engine::HeadlessOutput::deterministic();
    let old = SurfaceId::new(91, 1);
    let current = SurfaceId::new(91, 2);
    let mut layout = PersistentLiveLayout::default();
    layout.layers.insert(current, layer(current));
    assert_eq!(
        fixture
            .wm
            .enqueue_focus(old, &layout, output)
            .expect("a retired surface identity is stale input"),
        LiveWmRequestAdmission::Duplicate,
    );
    assert!(fixture.wm.public.as_ref().unwrap().queue.is_empty());

    // A new click on the exact current identity still reaches policy, once.
    assert_eq!(
        fixture.wm.enqueue_focus(current, &layout, output).unwrap(),
        LiveWmRequestAdmission::Admitted
    );
    assert_eq!(
        fixture.wm.enqueue_focus(current, &layout, output).unwrap(),
        LiveWmRequestAdmission::Duplicate
    );
    let public = fixture.wm.public.as_ref().unwrap();
    assert_eq!(public.active_output, output.id);
    assert_eq!(public.queue.len(), 1);
    assert_eq!(
        public.queue[0].cause,
        sophia_protocol::PolicyRequestCause::Focus { target: current }
    );
}

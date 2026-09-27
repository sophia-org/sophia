use sophia_protocol::*;

use super::Peer;

pub(super) fn tabs(peer: &mut Peer) {
    for superseded in [true, false] {
        let frames = peer.transfer(
            IpcMessageKind::ShellTabsBegin,
            IpcMessageKind::ShellTabsEnd,
            2 + SOPHIA_SHELL_MAX_TAB_GROUPS + SOPHIA_SHELL_MAX_TAB_ENTRIES,
        );
        let (tx, snapshot) = decode_shell_tab_snapshot(&frames).unwrap();
        assert_eq!(snapshot.connection_epoch, peer.epoch);
        let generation = peer.next_generation();
        peer.send(
            encode_shell_tab_candidate(
                tx,
                &ShellTabCandidate {
                    connection_epoch: peer.epoch,
                    snapshot_generation: snapshot.generation,
                    candidate_generation: generation,
                    groups: snapshot.groups.iter().map(|g| g.slot).collect(),
                },
            )
            .unwrap(),
        );
        if superseded {
            peer.outcome(tx, generation, ShellV1CandidateOutcomeKind::Superseded);
        } else {
            peer.outcome(tx, generation, ShellV1CandidateOutcomeKind::Prepared);
            let presented = peer.outcome(tx, generation, ShellV1CandidateOutcomeKind::Presented);
            let actions: Vec<_> = snapshot
                .groups
                .iter()
                .flat_map(|g| &g.entries)
                .map(|e| e.action)
                .collect();
            assert_eq!(
                peer.activation(generation, presented.presentation_epoch, &actions),
                ShellV1ActivationDisposition::Consumed
            );
            assert_eq!(
                peer.activation(generation, presented.presentation_epoch, &actions),
                ShellV1ActivationDisposition::RejectedStale
            );
        }
    }
}

pub(super) fn reference(peer: &mut Peer) {
    let frames = peer.transfer(
        IpcMessageKind::ShellShortcutsBegin,
        IpcMessageKind::ShellShortcutsEnd,
        SOPHIA_SHELL_MAX_SHORTCUTS + 2,
    );
    let (_, catalog) = decode_shell_shortcut_catalog(&frames).unwrap();
    assert_eq!(catalog.connection_epoch, peer.epoch);
    let mut page = 0;
    let mut pages = 1;
    let mut presented = 0;
    let mut visible = false;
    for expected in [
        ShellReferenceOperation::Startup,
        ShellReferenceOperation::Next,
        ShellReferenceOperation::Previous,
        ShellReferenceOperation::Dismiss,
        ShellReferenceOperation::Toggle,
    ] {
        let (tx, request) = decode_shell_reference_request(&peer.read()).unwrap();
        assert_eq!(request.operation, expected);
        assert_eq!(request.connection_epoch, peer.epoch);
        assert_eq!(request.catalog_generation, catalog.generation);
        assert_eq!(request.presentation_epoch, presented);
        let next_page = match request.operation {
            ShellReferenceOperation::Next => (page + 1) % pages,
            ShellReferenceOperation::Previous => (page + pages - 1) % pages,
            _ => page,
        };
        visible = match request.operation {
            ShellReferenceOperation::Startup => true,
            ShellReferenceOperation::Dismiss => false,
            ShellReferenceOperation::Toggle => !visible,
            _ => visible,
        };
        let generation = peer.next_generation();
        peer.send(
            encode_shell_reference_candidate(
                tx,
                &ShellReferenceCandidate {
                    connection_epoch: peer.epoch,
                    catalog_generation: catalog.generation,
                    request_generation: request.request_generation,
                    candidate_generation: generation,
                    output: request.output,
                    visible,
                    page: next_page,
                    style: ShellReferenceStyle {
                        body_size: 14,
                        title_size: 18,
                        padding: 8,
                        row_gap: 4,
                        key_gap: 8,
                        column_gap: 16,
                        border: 1,
                        margin: 8,
                        columns: 2,
                        colors: [
                            0xff202020, 0xffffffff, 0xffeeeeee, 0xffdddddd, 0xffcccccc, 0xffbbbbbb,
                        ],
                        title: "Contract fixture".into(),
                    },
                    entries: catalog
                        .entries
                        .iter()
                        .map(|e| ShellReferenceEntry {
                            slot: e.slot,
                            key: e.chord.clone(),
                            label: e.label.clone().unwrap_or_else(|| e.action.clone()),
                        })
                        .collect(),
                },
            )
            .unwrap(),
        );
        for kind in [
            ShellV1CandidateOutcomeKind::Prepared,
            ShellV1CandidateOutcomeKind::Presented,
        ] {
            let (actual, outcome) = decode_shell_reference_outcome(&peer.read()).unwrap();
            assert_eq!(actual, tx);
            assert_eq!(outcome.connection_epoch, peer.epoch);
            assert_eq!(outcome.catalog_generation, catalog.generation);
            assert_eq!(outcome.request_generation, request.request_generation);
            assert_eq!(outcome.candidate_generation, generation);
            assert_eq!(outcome.kind, kind);
            assert!(outcome.pages > 0 && outcome.page < outcome.pages);
            if kind == ShellV1CandidateOutcomeKind::Presented {
                assert!(outcome.presentation_epoch > presented);
                presented = outcome.presentation_epoch;
                page = outcome.page;
                pages = outcome.pages;
            } else {
                assert_eq!(outcome.presentation_epoch, 0);
            }
        }
    }
}

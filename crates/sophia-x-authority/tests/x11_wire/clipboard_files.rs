use super::*;
use sophia_portal::execution_fence::PortalExecutionFence;
use std::{
    io::Write,
    num::NonZeroUsize,
    sync::{Arc, mpsc},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

struct Policy {
    source: NamespaceContext,
    target: NamespaceContext,
    allow: bool,
}
impl XClipboardPortalPolicy for Policy {
    fn namespace(&self, id: NamespaceId) -> Option<NamespaceContext> {
        [self.source, self.target].into_iter().find(|n| n.id == id)
    }
    fn allow(&self, request: &PortalRequest) -> bool {
        self.allow
            && request.transfer.source_namespace == self.source.id
            && request.transfer.target_namespace == self.target.id
            && request.transfer.kind == PortalTransferKind::Clipboard
    }
}
struct Running {
    stop: mpsc::SyncSender<XServerFrontendServiceCommand>,
    server: Option<std::thread::JoinHandle<Result<(), X11SetupSocketError>>>,
    sink: Option<std::thread::JoinHandle<()>>,
    paths: Vec<std::path::PathBuf>,
}
impl Drop for Running {
    fn drop(&mut self) {
        let _ = self
            .stop
            .try_send(XServerFrontendServiceCommand::StopAndDisconnect);
        if let Some(server) = self.server.take() {
            let result = server.join();
            if !std::thread::panicking() {
                result.unwrap().unwrap();
            }
        }
        if let Some(sink) = self.sink.take() {
            let _ = sink.join();
        }
        for path in &self.paths {
            let _ = std::fs::remove_file(path);
        }
    }
}
#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Pass,
    InspectProxy,
    WrongType,
    WrongFormat,
    Targets,
    Deny,
    MissingRights,
    LockCycle,
    SourceChanged,
    SourceDisconnected,
    MissingPayload,
    Late,
}

fn joined(selection_name: &str, mode: Mode) {
    let serial = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("p9clip-{}-{serial}", std::process::id()));
    let target_path = path.with_extension("group");
    let source = NamespaceContext::new(
        NamespaceId::from_raw(860),
        NamespaceProfile::Confined,
        NamespaceCapabilities::NONE.with_publish(NamespacePortalCapability::Clipboard),
    )
    .unwrap();
    let target = NamespaceContext::new(
        NamespaceId::from_raw(861),
        NamespaceProfile::ClassicShared,
        if mode == Mode::MissingRights {
            NamespaceCapabilities::NONE
        } else {
            NamespaceCapabilities::NONE.with_request(NamespacePortalCapability::Clipboard)
        },
    )
    .unwrap();
    let fence = PortalExecutionFence::default();
    let config = XServerFrontendConfig::new_with_namespace_context(&path, source)
        .unwrap()
        .with_listener(XServerFrontendListenerId(7), &target_path, target)
        .unwrap()
        .with_admission_policy(Arc::new(ListenerXAdmissionPolicy::new(source, target)))
        .with_clipboard_portal(XClipboardPortal {
            broker_generation: 1,
            policy: Arc::new(Policy {
                source,
                target,
                allow: mode != Mode::Deny,
            }),
            fence: fence.clone(),
        });
    let (transactions, receive) = mpsc::sync_channel(4);
    let (stop, commands) = mpsc::sync_channel(1);
    let _running = Running {
        stop,
        server: Some(std::thread::spawn(move || {
            run_x_server_frontend_routed_until_stopped(
                config,
                transactions,
                XServerFrontendRouteBroker::new(NonZeroUsize::new(8).unwrap()),
                commands,
            )
        })),
        sink: Some(std::thread::spawn(move || while receive.recv().is_ok() {})),
        paths: vec![path.clone(), target_path.clone()],
    };
    wait_for_socket(&path);
    wait_for_socket(&target_path);
    let setup = |path: &std::path::Path| {
        let mut stream = connect_x_socket(path);
        stream
            .set_read_timeout(Some(Duration::from_secs(4)))
            .unwrap();
        stream
            .write_all(&setup_request(XByteOrder::LittleEndian, 11, 0, b"", b""))
            .unwrap();
        let window = read_setup_resource_id_base(&mut stream, XByteOrder::LittleEndian) + 1;
        stream
            .write_all(&create_window_request(
                XByteOrder::LittleEndian,
                window,
                0,
                0,
                100,
                60,
            ))
            .unwrap();
        (stream, window)
    };
    let (mut owner, owner_window) = setup(&path);
    let mut atom = |name| {
        owner
            .write_all(&intern_atom_request(XByteOrder::LittleEndian, false, name))
            .unwrap();
        read_u32(XByteOrder::LittleEndian, &read_x_record(&mut owner)[8..12])
    };
    let utf8 = atom("UTF8_STRING");
    let targets = atom("TARGETS");
    let text_utf8 = atom("text/plain;charset=utf-8");
    let selection = atom(selection_name);
    let property = atom("PORTAL_FILES_TEST");
    owner
        .write_all(&set_selection_owner_request(
            XByteOrder::LittleEndian,
            owner_window,
            selection,
            10,
        ))
        .unwrap();
    owner
        .write_all(&resource_request(XByteOrder::LittleEndian, 23, selection))
        .unwrap();
    assert_eq!(read_x_record(&mut owner)[0], 1);
    let (mut requestor, window) = setup(&target_path);
    for round in 0..if matches!(mode, Mode::Pass | Mode::MissingPayload) {
        2
    } else {
        1
    } {
        // A source capture failure must leave both clients and the files
        // coordinator usable for the following conversion.
        let mode = if round == 1 { Mode::Pass } else { mode };
        requestor
            .write_all(&convert_selection_request(
                XByteOrder::LittleEndian,
                window,
                selection,
                if mode == Mode::Targets { targets } else { utf8 },
                property,
                11,
            ))
            .unwrap();
        if !matches!(mode, Mode::Deny | Mode::MissingRights | Mode::Targets) {
            let event = read_x_record(&mut owner);
            assert_eq!(
                event[0], 30,
                "production coordinator must request source bytes"
            );
            if mode == Mode::LockCycle {
                fence.block();
                fence.unblock();
            }
            if mode == Mode::Late {
                std::thread::sleep(Duration::from_millis(2100));
            }
            if mode == Mode::SourceChanged {
                owner
                    .write_all(&set_selection_owner_request(
                        XByteOrder::LittleEndian,
                        owner_window,
                        selection,
                        12,
                    ))
                    .unwrap();
                owner
                    .write_all(&resource_request(XByteOrder::LittleEndian, 23, selection))
                    .unwrap();
                assert_eq!(read_x_record(&mut owner)[0], 1);
            }
            let proxy = read_u32(XByteOrder::LittleEndian, &event[12..16]);
            let proxy_property = read_u32(XByteOrder::LittleEndian, &event[24..28]);
            if mode == Mode::InspectProxy {
                // A selection owner may inspect a foreign requestor before
                // handing it data. XGetWindowAttributes also asks GetGeometry.
                for opcode in [3, 14, 15] {
                    owner
                        .write_all(&resource_request(XByteOrder::LittleEndian, opcode, proxy))
                        .unwrap();
                    let reply = read_x_record(&mut owner);
                    assert_eq!(
                        reply[0], 1,
                        "clipboard requestor inspection must succeed before source delivery (opcode {opcode})"
                    );
                    let remaining = read_u32(XByteOrder::LittleEndian, &reply[4..8]) as usize * 4;
                    std::io::Read::read_exact(&mut owner, &mut vec![0; remaining]).unwrap();
                }
                requestor
                    .write_all(&resource_request(XByteOrder::LittleEndian, 14, proxy))
                    .unwrap();
                let refused = read_x_record(&mut requestor);
                assert_eq!(
                    &refused[..2],
                    &[0, 10],
                    "another namespace cannot inspect the source endpoint"
                );
                owner
                    .write_all(&shape_select_input_request(
                        XByteOrder::LittleEndian,
                        proxy,
                        true,
                    ))
                    .unwrap();
                let refused = read_x_record(&mut owner);
                assert_eq!(
                    refused[0], 0,
                    "a clipboard endpoint is not a general extension window"
                );
                owner
                    .write_all(&send_selection_notify_request(
                        XByteOrder::LittleEndian,
                        proxy,
                        read_u32(XByteOrder::LittleEndian, &event[4..8]) + 1,
                        selection,
                        utf8,
                        proxy_property,
                    ))
                    .unwrap();
                let refused = read_x_record(&mut owner);
                assert_eq!(
                    &refused[..2],
                    &[0, 10],
                    "an uncorrelated notification cannot consume the endpoint"
                );
                owner
                    .write_all(&change_window_event_mask_request(
                        XByteOrder::LittleEndian,
                        proxy,
                        (1 << 22) | (1 << 17),
                    ))
                    .unwrap();
            }
            if mode == Mode::SourceDisconnected {
                owner.shutdown(std::net::Shutdown::Both).unwrap();
            } else {
                if mode != Mode::MissingPayload {
                    owner
                        .write_all(&change_property_request(
                            XByteOrder::LittleEndian,
                            XPropertyMode::Replace,
                            proxy,
                            proxy_property,
                            if mode == Mode::WrongType {
                                X_ATOM_ATOM
                            } else {
                                utf8
                            },
                            if mode == Mode::WrongFormat { 16 } else { 8 },
                            b"files-path",
                        ))
                        .unwrap();
                }
                owner
                    .write_all(&send_selection_notify_request(
                        XByteOrder::LittleEndian,
                        proxy,
                        read_u32(XByteOrder::LittleEndian, &event[4..8]),
                        selection,
                        utf8,
                        proxy_property,
                    ))
                    .unwrap();
            }
        }
        let notify = read_x_record(&mut requestor);
        assert_eq!(
            notify[0], 31,
            "bounded transfer must end with SelectionNotify"
        );
        assert_eq!(
            read_u32(XByteOrder::LittleEndian, &notify[20..24]),
            if matches!(mode, Mode::Pass | Mode::InspectProxy | Mode::Targets) {
                property
            } else {
                0
            }
        );
        requestor
            .write_all(&get_property_request(
                XByteOrder::LittleEndian,
                false,
                window,
                property,
                X_PROPERTY_ANY_TYPE,
                0,
                u32::MAX,
            ))
            .unwrap();
        let reply = read_x_reply(&mut requestor, XByteOrder::LittleEndian);
        if matches!(mode, Mode::Pass | Mode::InspectProxy) {
            assert_eq!(
                &reply[32..42],
                b"files-path",
                "real X recipient must install the file-uploaded bytes"
            );
            if mode == Mode::InspectProxy {
                let changed = read_x_record(&mut owner);
                assert_eq!(
                    changed[0], 28,
                    "the temporary endpoint supports its bounded property subscription"
                );
                let proxy = read_u32(XByteOrder::LittleEndian, &changed[4..8]);
                owner
                    .write_all(&resource_request(XByteOrder::LittleEndian, 14, proxy))
                    .unwrap();
                let retired = read_x_record(&mut owner);
                assert_eq!(
                    retired[0], 0,
                    "a completed source endpoint cannot be inspected or reused"
                );
            }
        } else if mode == Mode::Targets {
            assert_eq!(reply[1], 32, "TARGETS uses 32-bit atoms");
            assert_eq!(
                read_u32(XByteOrder::LittleEndian, &reply[8..12]),
                X_ATOM_ATOM
            );
            assert_eq!(read_u32(XByteOrder::LittleEndian, &reply[16..20]), 3);
            let advertised: Vec<_> = reply[32..44]
                .chunks_exact(4)
                .map(|bytes| read_u32(XByteOrder::LittleEndian, bytes))
                .collect();
            assert_eq!(
                advertised,
                [targets, utf8, text_utf8],
                "the X recipient translates TARGETS through the files grant"
            );
        } else {
            assert_eq!(
                read_u32(XByteOrder::LittleEndian, &reply[8..12]),
                0,
                "refusal must leave no property"
            );
        }
    }
}
#[test]
fn files_coordinator_transfers_clipboard() {
    joined("CLIPBOARD", Mode::Pass);
}
#[test]
fn files_coordinator_transfers_primary() {
    joined("PRIMARY", Mode::Pass);
}
#[test]
fn files_coordinator_source_inspects_requestor_before_reply() {
    for selection in ["CLIPBOARD", "PRIMARY"] {
        joined(selection, Mode::InspectProxy);
    }
}
#[test]
fn files_coordinator_refuses_source_type_and_format_mismatches() {
    joined("CLIPBOARD", Mode::WrongType);
    joined("PRIMARY", Mode::WrongFormat);
}
#[test]
fn files_coordinator_translates_targets_without_fetching_source_bytes() {
    joined("CLIPBOARD", Mode::Targets);
}
#[test]
fn files_coordinator_requires_policy() {
    joined("CLIPBOARD", Mode::Deny);
}
#[test]
fn files_coordinator_requires_participation() {
    joined("CLIPBOARD", Mode::MissingRights);
}
#[test]
fn files_coordinator_lock_then_unlock_never_revives_work() {
    joined("CLIPBOARD", Mode::LockCycle);
}
#[test]
fn files_coordinator_refuses_replaced_source() {
    joined("CLIPBOARD", Mode::SourceChanged);
}

#[test]
fn files_coordinator_expires_a_late_source() {
    joined("CLIPBOARD", Mode::Late);
}

#[test]
fn files_coordinator_refuses_disconnected_clipboard_source() {
    joined("CLIPBOARD", Mode::SourceDisconnected);
}

#[test]
fn files_coordinator_refuses_disconnected_primary_source() {
    joined("PRIMARY", Mode::SourceDisconnected);
}

#[test]
fn files_coordinator_refuses_clipboard_reply_without_payload() {
    joined("CLIPBOARD", Mode::MissingPayload);
}

#[test]
fn files_coordinator_refuses_primary_reply_without_payload() {
    joined("PRIMARY", Mode::MissingPayload);
}

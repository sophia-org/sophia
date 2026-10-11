use super::*;
#[path = "portal_object.rs"]
mod object;
use sophia_portal::{
    coordinator::*, execution_fence::PortalExecutionFence, handlers::ClipboardPayloadHandler,
    recipient_object::PortalObjectRecipient,
};
use sophia_protocol::{PortalTransferId, portal_files::PortalFileLimits};
use std::{
    io::Write,
    num::NonZeroUsize,
    sync::{Arc, Mutex, mpsc},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

struct Policy;
impl PortalPolicy for Policy {
    fn namespace(&self, id: NamespaceId) -> Option<NamespaceContext> {
        [1, 2].contains(&id.raw()).then(|| {
            NamespaceContext::new(id, NamespaceProfile::Confined, NamespaceCapabilities::ALL)
                .unwrap()
        })
    }
    fn allow(&self, request: &PortalRequest) -> bool {
        request.transfer.source_namespace.raw() == 1 && request.transfer.target_namespace.raw() == 2
    }
}
struct NinePSource(Mutex<object::Wire>);
impl PortalSource for NinePSource {
    fn generation(&self, r: &PortalRequest) -> Option<u64> {
        (r.transfer.source_namespace.raw() == 1).then_some(9)
    }
    fn collect(
        &self,
        _: &PortalRequest,
        _: &PortalGrant,
        wait: &PortalCollection,
    ) -> Result<Vec<u8>, PortalTransferFailure> {
        wait.remaining()?;
        self.0
            .lock()
            .unwrap()
            .read()
            .map_err(|_| PortalTransferFailure::Source)
    }
}
fn socket_path() -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "pa-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}
fn connect(path: &std::path::Path) -> (std::os::unix::net::UnixStream, u32) {
    wait_for_socket(path);
    let mut stream = connect_x_socket(path);
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
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
            40,
            40,
        ))
        .unwrap();
    (stream, window)
}
fn atom(stream: &mut std::os::unix::net::UnixStream, name: &str) -> u32 {
    stream
        .write_all(&intern_atom_request(XByteOrder::LittleEndian, false, name))
        .unwrap();
    read_u32(XByteOrder::LittleEndian, &read_x_record(stream)[8..12])
}
fn coordinator(
    source: Arc<dyn PortalSource>,
    recipient: Arc<dyn PortalRecipient>,
    fence: PortalExecutionFence,
) -> PortalTransferCoordinator {
    PortalTransferCoordinator::start(
        Arc::new(Policy),
        source,
        recipient,
        Arc::new(ClipboardPayloadHandler),
        fence,
        PortalFileLimits::defaults(1),
    )
    .unwrap()
}

#[test]
fn x_source_to_9p_recipient_uses_no_x_recipient_and_retains_a_revocable_binding() {
    for selection_name in ["CLIPBOARD", "PRIMARY"] {
        let path = socket_path();
        let config = XServerFrontendConfig::new_with_namespace_context(
            &path,
            Policy.namespace(NamespaceId::from_raw(1)).unwrap(),
        )
        .unwrap();
        let (send, receive) = mpsc::sync_channel(1);
        let server = std::thread::spawn(move || {
            let mut broker = XServerFrontendRouteBroker::new(NonZeroUsize::new(4).unwrap());
            let mut frontend = XServerFrontend::bind(config).unwrap();
            let source = frontend
                .clipboard_executor(&broker)
                .source_adapter(&mut broker, NamespaceId::from_raw(1), selection_name)
                .unwrap();
            send.send(Arc::new(source)).unwrap();
            frontend
                .serve_next_concurrently_routed_traced(&broker, Arc::new(|_| Ok(None)))
                .unwrap();
            frontend.wait_for_clients().unwrap();
        });
        let source = receive.recv_timeout(Duration::from_secs(2)).unwrap();
        let (mut owner, window) = connect(&path);
        let utf8 = atom(&mut owner, "UTF8_STRING");
        let selection = atom(&mut owner, selection_name);
        owner
            .write_all(&set_selection_owner_request(
                XByteOrder::LittleEndian,
                window,
                selection,
                10,
            ))
            .unwrap();
        owner
            .write_all(&resource_request(XByteOrder::LittleEndian, 23, selection))
            .unwrap();
        assert_eq!(read_x_record(&mut owner)[0], 1);
        let recipient = Arc::new(PortalObjectRecipient::new(NamespaceId::from_raw(2)));
        let fence = PortalExecutionFence::default();
        let common = coordinator(source, recipient.clone(), fence.clone());
        let completion = common.enqueue(PortalTransfer {
            transfer: PortalTransferId::from_raw(100),
            source_namespace: NamespaceId::from_raw(1),
            target_namespace: NamespaceId::from_raw(2),
            kind: PortalTransferKind::Clipboard,
            mime_type: Some("UTF8_STRING".into()),
            byte_size: 0,
            decision: PortalDecision::Pending,
            generation: 1,
        });
        let request = read_x_record(&mut owner);
        assert_eq!(request[0], 30);
        let proxy = read_u32(XByteOrder::LittleEndian, &request[12..16]);
        let property = read_u32(XByteOrder::LittleEndian, &request[24..28]);
        owner
            .write_all(&change_property_request(
                XByteOrder::LittleEndian,
                XPropertyMode::Replace,
                proxy,
                property,
                utf8,
                8,
                b"from X",
            ))
            .unwrap();
        owner
            .write_all(&send_selection_notify_request(
                XByteOrder::LittleEndian,
                proxy,
                read_u32(XByteOrder::LittleEndian, &request[4..8]),
                selection,
                utf8,
                property,
            ))
            .unwrap();
        assert_eq!(
            completion.recv_timeout(Duration::from_secs(3)).unwrap(),
            Ok(())
        );
        let mut wire = object::Wire::new(object::Data::Bound(recipient));
        wire.open();
        assert_eq!(
            wire.read().unwrap(),
            b"from X",
            "completed cross-protocol bind stays readable"
        );
        fence.block();
        fence.unblock();
        assert_eq!(
            wire.read(),
            Err(116),
            "lock cannot revive a retained 9P fid"
        );
        drop(common);
        drop(owner);
        server.join().unwrap();
        std::fs::remove_file(path).unwrap();
    }
}

#[test]
fn ninep_source_to_x_recipient_uses_registered_offer_and_real_selection_request() {
    for selection_name in ["CLIPBOARD", "PRIMARY"] {
        let path = socket_path();
        let config = XServerFrontendConfig::new_with_namespace_context(
            &path,
            Policy.namespace(NamespaceId::from_raw(2)).unwrap(),
        )
        .unwrap();
        let (send, receive) = mpsc::sync_channel(1);
        let (requests, requested) = mpsc::sync_channel(1);
        let server = std::thread::spawn(move || {
            let broker = XServerFrontendRouteBroker::new(NonZeroUsize::new(4).unwrap());
            let mut frontend = XServerFrontend::bind(config).unwrap();
            let executor = frontend.clipboard_executor(&broker);
            executor
                .register_portal_selection(
                    selection_name,
                    XPortalSelectionOffer {
                        source_namespace: NamespaceId::from_raw(1),
                        generation: 9,
                    },
                )
                .unwrap();
            send.send(executor).unwrap();
            let observer: Arc<X11CoreTraceObserver> = Arc::new(move |trace| {
                if let Some(response) = &trace.result.response {
                    for command in &response.portal_commands {
                        if let XAuthorityPortalCommand::PromptClipboardTransfer(transfer) = command
                        {
                            requests.try_send(transfer.clone()).unwrap();
                        }
                    }
                }
                Ok(None)
            });
            frontend
                .serve_next_concurrently_routed_traced(&broker, observer)
                .unwrap();
            frontend.wait_for_clients().unwrap();
        });
        let recipient = Arc::new(receive.recv_timeout(Duration::from_secs(2)).unwrap());
        let mut wire = object::Wire::new(object::Data::Source(b"from 9P".to_vec()));
        wire.open();
        let source = Arc::new(NinePSource(Mutex::new(wire)));
        let common = coordinator(source.clone(), recipient, PortalExecutionFence::default());
        let (mut requestor, window) = connect(&path);
        let utf8 = atom(&mut requestor, "UTF8_STRING");
        let selection = atom(&mut requestor, selection_name);
        let property = atom(&mut requestor, "PORTAL_RESULT");
        requestor
            .write_all(&convert_selection_request(
                XByteOrder::LittleEndian,
                window,
                selection,
                utf8,
                property,
                0,
            ))
            .unwrap();
        let transfer = requested.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(transfer.source_namespace.raw(), 1);
        let completion = common.enqueue(transfer);
        let notify = read_x_record(&mut requestor);
        assert_eq!(notify[0], 31);
        assert_eq!(
            read_u32(XByteOrder::LittleEndian, &notify[20..24]),
            property
        );
        assert_eq!(
            completion.recv_timeout(Duration::from_secs(3)).unwrap(),
            Ok(())
        );
        requestor
            .write_all(&get_property_request(
                XByteOrder::LittleEndian,
                true,
                window,
                property,
                X_PROPERTY_ANY_TYPE,
                0,
                1024,
            ))
            .unwrap();
        let reply = read_x_reply(&mut requestor, XByteOrder::LittleEndian);
        assert_eq!(&reply[32..39], b"from 9P");
        assert_eq!(
            source.0.lock().unwrap().owner.reads,
            1,
            "source is read once through the 9P core"
        );
        drop(common);
        drop(requestor);
        server.join().unwrap();
        std::fs::remove_file(path).unwrap();
    }
}

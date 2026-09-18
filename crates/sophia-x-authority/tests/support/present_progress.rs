//! Production route registry and socket writer; completions are supplied, not
//! native/KMS evidence. A held buffer must not stall the other windows.
use super::*;
use std::io::Read;

fn read_event(peer: &mut UnixStream, order: XByteOrder) -> Vec<u8> {
    let mut bytes = vec![0; 32];
    peer.read_exact(&mut bytes).unwrap();
    assert_eq!(bytes[0], 35);
    let extra = order.u32(&bytes[4..8]) as usize * 4;
    assert!(extra <= 8);
    bytes.resize(32 + extra, 0);
    peer.read_exact(&mut bytes[32..]).unwrap();
    bytes
}

#[test]
fn three_windows_reuse_buffers_while_one_exact_present_remains_held() {
    for order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
        let owner = XServerFrontendClientId::from_raw(1);
        let gpu = XServerFrontendClientId::from_raw(2);
        let broker = XServerFrontendRouteBroker::new(NonZeroUsize::new(16).unwrap());
        let (_owner, owner_channels) = broker.registry.register_client(owner).unwrap();
        let (_gpu, channels) = broker.registry.register_client(gpu).unwrap();
        let (stream, mut peer) = UnixStream::pair().unwrap();
        peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        let writer = spawn_x11_protocol_event_writer(
            Arc::new(Mutex::new(stream)),
            Arc::new(AtomicUsize::new(0)),
            order,
            Arc::new(AtomicU16::new(17)),
            gpu,
            channels.protocol,
        )
        .unwrap();
        let windows = [0x300010, 0x300020, 0x300030].map(|id| XResourceId::new(id, 1));
        for (index, window) in windows.iter().copied().enumerate() {
            broker
                .registry
                .register_surface(
                    owner,
                    NamespaceId::from_raw(1),
                    SurfaceId::new(index as u32 + 1, 1),
                    window,
                )
                .unwrap();
            broker
                .registry
                .select_present_input(gpu, XResourceId::new(0x400010 + index as u64, 1), window, 6)
                .unwrap();
        }
        let held = TransactionId::from_raw(1);
        broker
            .registry
            .queue_present(
                held,
                gpu,
                windows[0],
                XResourceId::new(0x500001, 1),
                1,
                None,
                false,
            )
            .unwrap();
        for cycle in 0..1000_u32 {
            for (index, window) in windows.iter().copied().enumerate() {
                let serial = 2 + cycle * 3 + index as u32;
                let transaction = TransactionId::from_raw(u64::from(serial));
                let pixmap =
                    XResourceId::new(0x500010 + index as u64 * 2 + u64::from(cycle % 2), 1);
                broker
                    .registry
                    .queue_present(transaction, gpu, window, pixmap, serial, None, false)
                    .unwrap();
                let idle_first = cycle % 2 == 0;
                if idle_first {
                    assert_eq!(broker.route_present_idle(transaction), Ok(true));
                }
                assert_eq!(
                    broker.route_present_complete(
                        transaction,
                        u64::from(serial),
                        u64::from(serial),
                        XPresentCompletionMode::Copy
                    ),
                    Ok(true)
                );
                if !idle_first {
                    assert_eq!(broker.route_present_idle(transaction), Ok(true));
                }
                for kind in if idle_first { [2, 1] } else { [1, 2] } {
                    let event = read_event(&mut peer, order);
                    assert_eq!(order.u16(&event[8..10]), kind);
                    assert_eq!(order.u16(&event[2..4]), 17);
                    assert_eq!(order.u32(&event[12..16]), 0x400010 + index as u32);
                    assert_eq!(order.u32(&event[16..20]), window.local.raw() as u32);
                    assert_eq!(order.u32(&event[20..24]), serial);
                    if kind == 2 {
                        assert_eq!(order.u32(&event[24..28]), pixmap.local.raw() as u32);
                    }
                }
                assert_eq!(broker.route_present_idle(transaction), Ok(false));
                assert_eq!(
                    broker
                        .registry
                        .pending_presentations
                        .entries
                        .lock()
                        .unwrap()
                        .len(),
                    1
                );
            }
        }
        assert_eq!(
            broker.route_present_complete(held, 4000, 4000, XPresentCompletionMode::Flip),
            Ok(true)
        );
        assert_eq!(broker.route_present_idle(held), Ok(true));
        read_event(&mut peer, order);
        read_event(&mut peer, order);
        assert!(
            broker
                .registry
                .pending_presentations
                .entries
                .lock()
                .unwrap()
                .is_empty()
        );
        assert!(owner_channels.protocol.try_recv().is_err());
        writer.stop.store(true, Ordering::Release);
        writer.thread.join().unwrap().unwrap();
    }
}

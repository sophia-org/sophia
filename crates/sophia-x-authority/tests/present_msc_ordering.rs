#![cfg(unix)]

use sophia_protocol::NamespaceId;
use sophia_x_authority::*;
use std::{
    io::{ErrorKind, Read, Write},
    num::NonZeroUsize,
    os::unix::net::UnixStream,
    path::PathBuf,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    thread::JoinHandle,
    time::Duration,
};

const WAIT: Duration = Duration::from_secs(10);
const SILENCE: Duration = Duration::from_millis(50);
static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    path: PathBuf,
    gate: Arc<(Mutex<bool>, Condvar)>,
    observed: mpsc::Receiver<u16>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<Result<(), X11SetupSocketError>>>,
}

impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "sophia-msc-order-{}-{}.sock",
            std::process::id(),
            NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed),
        ));
        let config = XServerFrontendConfig::new(&path, NamespaceId::from_raw(991))
            .unwrap()
            .with_max_concurrent_clients(NonZeroUsize::new(4).unwrap());
        let mut frontend = XServerFrontend::bind(config).unwrap();
        let broker = XServerFrontendRouteBroker::new(NonZeroUsize::new(16).unwrap());
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        let observed_gate = gate.clone();
        let (sender, observed) = mpsc::channel();
        let observer: Arc<X11CoreTraceObserver> = Arc::new(move |trace| {
            if trace.major_opcode == X_PRESENT_MAJOR_OPCODE
                && trace.minor_opcode == u16::from(X_PRESENT_NOTIFY_MSC_MINOR_OPCODE)
            {
                let _ = sender.send(trace.sequence);
                let (lock, changed) = &*observed_gate;
                let released = lock.lock().unwrap();
                drop(changed.wait_while(released, |released| !*released).unwrap());
            }
            Ok(None)
        });
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let worker = std::thread::spawn(move || {
            while !worker_stop.load(Ordering::Acquire) {
                frontend.try_serve_next_concurrently_routed_traced(&broker, observer.clone())?;
                frontend.poll_client_workers()?;
                std::thread::sleep(Duration::from_millis(1));
            }
            frontend.shutdown_all_client_workers()?;
            frontend.wait_for_clients()
        });
        Self {
            path,
            gate,
            observed,
            stop,
            worker: Some(worker),
        }
    }

    fn connect(&self, order: XByteOrder) -> Client {
        let mut stream = UnixStream::connect(&self.path).unwrap();
        stream.set_read_timeout(Some(WAIT)).unwrap();
        stream.set_write_timeout(Some(WAIT)).unwrap();
        let mut setup = vec![order.marker(), 0];
        for value in [11, 0, 0, 0, 0] {
            put16(&mut setup, order, value);
        }
        stream.write_all(&setup).unwrap();
        let mut header = [0; 8];
        stream.read_exact(&mut header).unwrap();
        assert_eq!(header[0], 1);
        let mut body = vec![0; usize::from(get16(order, &header[6..8])) * 4];
        stream.read_exact(&mut body).unwrap();
        Client {
            stream,
            order,
            sequence: 0,
            base: get32(order, &body[4..8]),
        }
    }

    fn wait_until_observed(&self, sequence: u16) {
        assert_eq!(self.observed.recv_timeout(WAIT).unwrap(), sequence);
    }

    fn release(&self) {
        let (lock, changed) = &*self.gate;
        *lock.lock().unwrap() = true;
        changed.notify_all();
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // Assertions may fail while the observer owns the request. Release it
        // before joining, including unwinding from a detected early event.
        self.release();
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let result = worker.join();
            if !std::thread::panicking() {
                result.unwrap().unwrap();
            }
        }
        let _ = std::fs::remove_file(&self.path);
    }
}

struct Client {
    stream: UnixStream,
    order: XByteOrder,
    sequence: u16,
    base: u32,
}

impl Client {
    fn send(&mut self, request: &[u8]) -> u16 {
        self.stream.write_all(request).unwrap();
        self.sequence = self.sequence.wrapping_add(1);
        self.sequence
    }

    fn header(&self, major: u8, minor: u8, words: u16) -> Vec<u8> {
        let mut request = vec![major, minor];
        put16(&mut request, self.order, words);
        request
    }

    fn record_with_timeout(&mut self, timeout: Duration) -> Option<Vec<u8>> {
        self.stream.set_read_timeout(Some(timeout)).unwrap();
        let mut first = [0];
        match self.stream.read_exact(&mut first) {
            Ok(()) => {}
            Err(error) if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                return None;
            }
            Err(error) => panic!("X record read failed: {error}"),
        }
        // Only the first byte uses the silence deadline. Once a record starts,
        // read it whole so a timeout cannot consume half of the next assertion.
        self.stream.set_read_timeout(Some(WAIT)).unwrap();
        let mut record = vec![0; 32];
        record[0] = first[0];
        self.stream.read_exact(&mut record[1..]).unwrap();
        if matches!(record[0], 1 | 35) {
            let extra = get32(self.order, &record[4..8]) as usize * 4;
            record.resize(32 + extra, 0);
            self.stream.read_exact(&mut record[32..]).unwrap();
        }
        Some(record)
    }

    fn record(&mut self) -> Vec<u8> {
        self.record_with_timeout(WAIT).expect("missing X record")
    }

    fn barrier(&mut self) {
        let request = self.header(43, 0, 1);
        let sequence = self.send(&request);
        self.assert_reply(sequence);
    }

    fn assert_reply(&mut self, sequence: u16) {
        let reply = self.record();
        assert_eq!(reply[0], 1, "expected reply, got {reply:?}");
        assert_eq!(get16(self.order, &reply[2..4]), sequence);
    }

    fn create_window(&mut self) -> u32 {
        let window = self.base + 1;
        let mut request = self.header(1, 0, 8);
        put32(&mut request, self.order, window);
        put32(&mut request, self.order, X_SETUP_DEFAULT_ROOT);
        for value in [0, 0, 16, 16, 0, 1] {
            put16(&mut request, self.order, value);
        }
        put32(&mut request, self.order, 0);
        put32(&mut request, self.order, 0);
        self.send(&request);
        self.barrier();
        window
    }

    fn subscribe(&mut self, window: u32) -> u32 {
        let event_id = self.base + 2;
        let mut request = self.header(
            X_PRESENT_MAJOR_OPCODE,
            X_PRESENT_SELECT_INPUT_MINOR_OPCODE,
            4,
        );
        for value in [event_id, window, 2] {
            put32(&mut request, self.order, value);
        }
        self.send(&request);
        self.barrier();
        event_id
    }

    fn notify_request(&self, window: u32, serial: u32) -> Vec<u8> {
        let mut request = self.header(
            X_PRESENT_MAJOR_OPCODE,
            X_PRESENT_NOTIFY_MSC_MINOR_OPCODE,
            10,
        );
        put32(&mut request, self.order, window);
        put32(&mut request, self.order, serial);
        request.resize(40, 0); // immediate target, no modulus
        request
    }

    fn assert_notify(&mut self, sequence: u16, event_id: u32, window: u32, serial: u32) {
        let event = self.record();
        assert_eq!(event[0], 35, "later reply overtook NotifyMSC: {event:?}");
        assert_eq!(event.len(), 40);
        assert_eq!(event[1], X_PRESENT_MAJOR_OPCODE);
        assert_eq!(
            get16(self.order, &event[2..4]),
            sequence,
            "Mesa's NotifyMSC cookie must match"
        );
        assert_eq!(get16(self.order, &event[8..10]), 1);
        assert_eq!(event[10], 1);
        assert_eq!(get32(self.order, &event[12..16]), event_id);
        assert_eq!(get32(self.order, &event[16..20]), window);
        assert_eq!(get32(self.order, &event[20..24]), serial);
    }
}

fn put16(out: &mut Vec<u8>, order: XByteOrder, value: u16) {
    out.extend(match order {
        XByteOrder::LittleEndian => value.to_le_bytes(),
        XByteOrder::BigEndian => value.to_be_bytes(),
    });
}
fn put32(out: &mut Vec<u8>, order: XByteOrder, value: u32) {
    out.extend(match order {
        XByteOrder::LittleEndian => value.to_le_bytes(),
        XByteOrder::BigEndian => value.to_be_bytes(),
    });
}
fn get16(order: XByteOrder, value: &[u8]) -> u16 {
    let bytes = value.try_into().unwrap();
    match order {
        XByteOrder::LittleEndian => u16::from_le_bytes(bytes),
        XByteOrder::BigEndian => u16::from_be_bytes(bytes),
    }
}
fn get32(order: XByteOrder, value: &[u8]) -> u32 {
    let bytes = value.try_into().unwrap();
    match order {
        XByteOrder::LittleEndian => u32::from_le_bytes(bytes),
        XByteOrder::BigEndian => u32::from_be_bytes(bytes),
    }
}

#[test]
fn immediate_notify_msc_waits_for_request_commit_and_has_its_sequence() {
    for order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
        let fixture = Fixture::new();
        let mut client = fixture.connect(order);
        let window = client.create_window();
        let event_id = client.subscribe(window);
        let sequence = client.send(&client.notify_request(window, 73));
        assert_eq!(sequence, 5);
        fixture.wait_until_observed(sequence);
        assert!(
            client.record_with_timeout(SILENCE).is_none(),
            "notification escaped the uncommitted request"
        );
        fixture.release();
        client.assert_notify(sequence, event_id, window, 73);
        client.barrier();
    }
}

#[test]
fn pipelined_replies_cannot_overtake_immediate_notify_msc() {
    for order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
        let fixture = Fixture::new();
        let mut client = fixture.connect(order);
        let window = client.create_window();
        let event_id = client.subscribe(window);
        let mut requests = Vec::new();
        for serial in 0..64 {
            requests.extend(client.notify_request(window, serial));
            requests.extend(client.header(43, 0, 1));
        }
        client.stream.write_all(&requests).unwrap();
        fixture.wait_until_observed(5);
        fixture.release();
        for serial in 0..64 {
            let sequence = 5 + serial as u16 * 2;
            client.assert_notify(sequence, event_id, window, serial);
            client.assert_reply(sequence + 1);
        }
    }
}

#[test]
fn immediate_notify_msc_serializes_across_the_sequence_wrap() {
    for order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
        let fixture = Fixture::new();
        let mut client = fixture.connect(order);
        let window = client.create_window();
        let event_id = client.subscribe(window);
        // Bell is a supported void request. It advances the real connection
        // sequence without widening production APIs to seed test state.
        let bell = client.header(104, 0, 1);
        let count = usize::from(u16::MAX - 1 - client.sequence);
        client.stream.write_all(&bell.repeat(count)).unwrap();
        client.sequence = u16::MAX - 1;
        client.barrier();
        let sequence = client.send(&client.notify_request(window, 74));
        assert_eq!(sequence, 0);
        fixture.wait_until_observed(sequence);
        assert!(
            client.record_with_timeout(SILENCE).is_none(),
            "wrapped notification escaped before sequence zero committed"
        );
        fixture.release();
        client.assert_notify(0, event_id, window, 74);
        client.barrier();
    }
}

#[test]
fn present_selection_after_destroy_reports_bad_window_and_keeps_serving() {
    for order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
        let fixture = Fixture::new();
        let mut client = fixture.connect(order);
        let window = client.create_window();
        let event_id = client.subscribe(window);
        let mut destroy = client.header(4, 0, 2);
        put32(&mut destroy, order, window);
        client.send(&destroy);

        // Teardown may unselect Present events after DestroyWindow has already
        // reached the server. Unrelated requests in between do not keep the
        // drawable alive, and the resulting error must remain request-local.
        let bell = client.header(104, 0, 1);
        client.send(&bell);
        client.send(&bell);
        let mut unselect = client.header(
            X_PRESENT_MAJOR_OPCODE,
            X_PRESENT_SELECT_INPUT_MINOR_OPCODE,
            4,
        );
        for value in [event_id, window, 0] {
            put32(&mut unselect, order, value);
        }
        let sequence = client.send(&unselect);
        let error = client.record();
        assert_eq!(&error[..2], &[0, XErrorCode::BadWindow.wire_code()]);
        assert_eq!(get16(order, &error[2..4]), sequence);
        assert_eq!(get32(order, &error[4..8]), window);
        assert_eq!(
            get16(order, &error[8..10]),
            u16::from(X_PRESENT_SELECT_INPUT_MINOR_OPCODE)
        );
        assert_eq!(error[10], X_PRESENT_MAJOR_OPCODE);

        // A reply and a new window on the same connection prove that the
        // failed unselection did not close or poison the client lane.
        client.barrier();
        assert_eq!(client.create_window(), window);
        assert!(client.record_with_timeout(SILENCE).is_none());
    }
}

#[test]
fn notify_msc_bad_window_reports_an_error_without_a_notification() {
    for order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
        let fixture = Fixture::new();
        let mut client = fixture.connect(order);
        let window = client.create_window();
        client.subscribe(window);
        let mut destroy = client.header(4, 0, 2);
        put32(&mut destroy, order, window);
        client.send(&destroy);
        let sequence = client.send(&client.notify_request(window, 75));
        fixture.wait_until_observed(sequence);
        fixture.release();
        let error = client.record();
        assert_eq!(&error[..2], &[0, XErrorCode::BadWindow.wire_code()]);
        assert_eq!(get16(order, &error[2..4]), sequence);
        assert_eq!(get32(order, &error[4..8]), window);
        assert_eq!(
            get16(order, &error[8..10]),
            u16::from(X_PRESENT_NOTIFY_MSC_MINOR_OPCODE)
        );
        client.barrier();
        assert!(client.record_with_timeout(SILENCE).is_none());
    }
}

#[test]
fn notify_msc_uses_each_subscribers_own_connection_sequence() {
    for order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
        let fixture = Fixture::new();
        let mut requester = fixture.connect(order);
        let window = requester.create_window();
        let request_event = requester.subscribe(window);
        let mut subscriber = fixture.connect(order);
        let subscriber_event = subscriber.subscribe(window);
        assert_eq!(subscriber.sequence, 2);
        let sequence = requester.send(&requester.notify_request(window, 76));
        fixture.wait_until_observed(sequence);
        assert!(requester.record_with_timeout(SILENCE).is_none());
        assert!(subscriber.record_with_timeout(SILENCE).is_none());
        fixture.release();
        requester.assert_notify(sequence, request_event, window, 76);
        subscriber.assert_notify(2, subscriber_event, window, 76);
        requester.barrier();
        subscriber.barrier();
    }
}

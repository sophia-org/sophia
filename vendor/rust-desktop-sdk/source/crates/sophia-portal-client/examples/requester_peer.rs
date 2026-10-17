//! Production conformance peer. The owner supplies an admitted socket on stdin.
use sophia_9p_client::pipeline::{Pipeline, PipelineLimits, Reply};
use sophia_portal_client::{
    protocol::requester::{TransferKind, TransferRequest},
    requester::{
        Error as StateError,
        session::{Error, Session},
        startup::Opening,
        transfer::State,
    },
};
use std::{
    os::fd::AsFd,
    os::unix::net::UnixStream,
    time::{Duration, Instant},
};

fn pump(s: &mut Session, start: Instant) {
    pump_budget(s, start, 65536);
}

fn pump_budget(s: &mut Session, start: Instant, budget: usize) {
    assert!(start.elapsed() < Duration::from_secs(8));
    let now = start.elapsed().as_millis() as u64;
    s.dispatch(now, budget).unwrap();
    while s.event().is_some() {
        s.consume(now).unwrap();
    }
    match s.acknowledge(now) {
        Ok(()) | Err(Error::State(StateError::Busy)) => (),
        result => panic!("{result:?}"),
    }
    std::thread::yield_now();
}

fn main() {
    let args: Vec<_> = std::env::args().collect();
    let mode = args[1].as_str();
    let length: usize = args[2].parse().unwrap();
    assert!(matches!(
        mode,
        "completed"
            | "revoked"
            | "denied"
            | "cancel"
            | "disconnect"
            | "lost-outcome"
            | "cancel-partial"
    ));
    assert!(length <= 65536);
    // This executable owns stdin exclusively; no ambient endpoint is opened.
    let stream = UnixStream::from(std::io::stdin().as_fd().try_clone_to_owned().unwrap());
    let mut wire =
        Pipeline::over(stream, PipelineLimits::default(), Duration::from_secs(2)).unwrap();
    let (tag, root) = wire.attach(b"", b"").unwrap();
    assert!(matches!(
        wire.wait(tag, Instant::now() + Duration::from_secs(2))
            .unwrap(),
        Reply::Attach(_)
    ));
    let start = Instant::now();
    let mut opening = Opening::from_attached(wire, root, 0, 8000).unwrap();
    while !opening.ready() {
        assert!(start.elapsed() < Duration::from_secs(8));
        opening
            .dispatch(start.elapsed().as_millis() as u64, 65536)
            .unwrap();
        std::thread::yield_now();
    }
    let opened = opening.into_opened().unwrap();
    let mut session = Session::new(opened, 1, 1, 8000).unwrap();
    while !session.transaction_ready() {
        pump(&mut session, start);
    }
    let id = session
        .request(
            TransferRequest {
                transfer: 1,
                source: 1,
                target: 2,
                kind: TransferKind::Clipboard,
                mime: Some("UTF8_STRING"),
                byte_size: length as u64,
                generation: 9,
                deadline_msec: 10000,
            },
            start.elapsed().as_millis() as u64,
            8000,
        )
        .unwrap();
    while !session.transaction_ready() {
        pump(&mut session, start);
    }
    if mode == "denied" {
        assert_eq!(session.transfer(id).unwrap().state(), State::Denied);
    } else {
        assert_eq!(session.transfer(id).unwrap().state(), State::Allowed);
        let mut bytes = vec![17; length];
        let pointer = bytes.as_ptr();
        let now = start.elapsed().as_millis() as u64;
        assert!(session.begin_buffer(id, &mut bytes, now, now).is_err());
        assert_eq!(bytes.as_ptr(), pointer);
        assert_eq!(bytes.len(), length);
        session
            .begin_buffer(id, &mut bytes, start.elapsed().as_millis() as u64, 8000)
            .unwrap();
        if mode == "cancel-partial" {
            while session.upload_written(id) == Some(0) || !session.transaction_ready() {
                pump_budget(&mut session, start, 1);
            }
            let written = session.upload_written(id).unwrap();
            assert!(written > 0 && written < length);
        } else {
            while !session.upload_ready(id) || !session.transaction_ready() {
                pump(&mut session, start);
            }
        }
        if mode == "disconnect" {
            session.close();
            let transfer = session.transfer(id).unwrap();
            assert_eq!(transfer.state(), State::Allowed);
            assert!(transfer.disconnected() && transfer.grant().is_none());
            let returned = session.take_upload(id).unwrap();
            assert_eq!(returned.as_ptr(), pointer);
            assert_eq!(returned.len(), length);
            return;
        }
        let now = start.elapsed().as_millis() as u64;
        if mode == "cancel" || mode == "cancel-partial" {
            session.cancel(id, now, 8000).unwrap();
        } else {
            session.end(id, now, 8000).unwrap();
        }
        if mode == "lost-outcome" {
            loop {
                assert!(start.elapsed() < Duration::from_secs(8));
                if session
                    .dispatch(start.elapsed().as_millis() as u64, 65536)
                    .is_err()
                {
                    break;
                }
                std::thread::yield_now();
            }
            let transfer = session.transfer(id).unwrap();
            assert_eq!(transfer.state(), State::EndIssued);
            assert!(transfer.disconnected() && transfer.grant().is_none());
            let returned = session.take_upload(id).unwrap();
            assert_eq!(returned.as_ptr(), pointer);
            assert_eq!(returned.len(), length);
            return;
        }
        let returned = loop {
            pump(&mut session, start);
            match session.take_upload(id) {
                Ok(bytes) => break bytes,
                Err(Error::State(StateError::Busy)) => (),
                result => panic!("{result:?}"),
            }
        };
        assert_eq!(returned.as_ptr(), pointer);
        assert_eq!(returned.len(), length);
        assert_eq!(
            session.transfer(id).unwrap().state(),
            if mode == "completed" {
                State::Completed
            } else {
                State::Revoked
            }
        );
    }
    loop {
        pump(&mut session, start);
        match session.release_transfer(id, start.elapsed().as_millis() as u64) {
            Ok(()) => break,
            Err(Error::State(StateError::Busy)) => (),
            result => panic!("{result:?}"),
        }
    }
    session.close();
}

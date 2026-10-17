//! Protected namespace startup peer; the fixture preserves the launch channel on stdin.
use sophia_portal_client::{protocol::RequestCapability, startup::Opening};
use std::{
    os::unix::net::UnixStream,
    time::{Duration, Instant},
};
fn main() {
    let deadline = Instant::now() + Duration::from_secs(10);
    let stdin = std::io::stdin();
    rustix::io::fcntl_setfd(&stdin, rustix::io::FdFlags::CLOEXEC).unwrap();
    let relay = UnixStream::from(rustix::io::fcntl_dupfd_cloexec(&stdin, 3).unwrap());
    let directory = std::env::var_os("SOPHIA_NAMESPACE_DIR").unwrap();
    let names = sophia_namespace_client::Namespace::open(std::path::Path::new(&directory)).unwrap();
    let mut auth = sophia_launch_client::Client::over(
        names.connect("roles/portal").unwrap(),
        relay,
        b"namespace-client",
        b"portal-client",
    )
    .unwrap();
    while !auth.ready() {
        assert!(Instant::now() < deadline);
        auth.poll().unwrap();
        if !auth.ready() {
            auth.wait_for_io(Duration::from_millis(10)).unwrap();
        }
    }
    let admitted = auth.into_authenticated().unwrap();
    let _relay = admitted.relay;
    let start = Instant::now();
    let mut opening = Opening::from_attached(admitted.role, admitted.root, 0, 2000).unwrap();
    while !opening.is_opened() {
        assert!(Instant::now() < deadline);
        let interest = opening.io_interest().unwrap();
        let mut flags = rustix::event::PollFlags::IN;
        if interest.writable {
            flags |= rustix::event::PollFlags::OUT;
        }
        let fd = opening.fd().unwrap();
        let mut fds = [rustix::event::PollFd::new(&fd, flags)];
        let now = start.elapsed().as_millis() as u64;
        let wait = Duration::from_millis(if interest.immediate {
            0
        } else {
            opening.deadline().unwrap().saturating_sub(now)
        });
        rustix::event::poll(
            &mut fds,
            Some(&rustix::event::Timespec::try_from(wait).unwrap()),
        )
        .unwrap();
        opening
            .dispatch(start.elapsed().as_millis() as u64, 65536)
            .unwrap();
    }
    let mut opened = opening.into_opened().unwrap();
    let mode = std::env::var("SOPHIA_TEST_PORTAL_BOUNDARY").unwrap();
    let request_mode = mode.starts_with("sdk-request");
    let refused = matches!(
        mode.as_str(),
        "sdk-request-refused" | "sdk-request-busy" | "sdk-request-stale"
    );
    let refusal_errno = match mode.as_str() {
        "sdk-request-busy" => 11,
        "sdk-request-stale" => 116,
        _ => 13,
    };
    let cancelled = mode.starts_with("sdk-request-cancelled");
    let session_capture = mode == "sdk-session-capture";
    let session_frame = mode == "sdk-session-frame";
    let frame_mode =
        mode.starts_with("sdk-frame") || request_mode || session_capture || session_frame;
    let revoke = mode == "sdk-frame-revoke";
    assert_eq!(
        opened.info.request,
        if frame_mode {
            RequestCapability::OutputCaptureV1
        } else {
            RequestCapability::Unavailable
        }
    );
    assert!(opened.info.epoch > 0 && opened.pipeline.fid_count() == 1);
    assert!(opened.pipeline.is_quiescent());
    if mode.starts_with("sdk-session") {
        use sophia_portal_client::{
            session::{Session, WatchEvent},
            snapshot::File,
        };
        let mut storage = vec![0xa5; 65536];
        let pointer = storage.as_ptr();
        let mut session = Session::new(opened).unwrap();
        let mut buffer = Some(storage.as_mut_slice());
        let expires = start.elapsed().as_millis() as u64 + 3000;
        session.watch(expires).unwrap();
        session.snapshot(File::Status, expires).unwrap();
        let mut phase = 0;
        let mut frame_cancelled = false;
        let mut image = Some(vec![0xa5; if session_frame { 256 * 128 * 4 } else { 0 }]);
        let image_pointer = image.as_ref().unwrap().as_ptr();
        loop {
            assert!(Instant::now() < deadline);
            let interest = session.io_interest().unwrap();
            let fd = session.fd().unwrap();
            let mut flags = rustix::event::PollFlags::IN;
            if interest.writable {
                flags |= rustix::event::PollFlags::OUT;
            }
            let mut fds = [rustix::event::PollFd::new(&fd, flags)];
            let wait = Duration::from_millis(if interest.immediate {
                0
            } else {
                session
                    .deadline()
                    .unwrap()
                    .saturating_sub(start.elapsed().as_millis() as u64)
            });
            rustix::event::poll(
                &mut fds,
                Some(&rustix::event::Timespec::try_from(wait).unwrap()),
            )
            .unwrap();
            session
                .dispatch(start.elapsed().as_millis() as u64, 65536)
                .unwrap();
            if phase == 0 && session.snapshot_result().is_some() {
                assert_eq!(
                    session.snapshot_result().unwrap(),
                    if session_capture || session_frame {
                        b"request=output-capture-v1 transfer=ready\n".as_slice()
                    } else {
                        b"request=unavailable transfer=ready\n".as_slice()
                    }
                );
                assert!(session.watch_waiting() && session.watch_event().is_none());
                session.release_snapshot().unwrap();
                let report = std::env::var_os("SOPHIA_TEST_LOCK_REPORT").unwrap();
                std::fs::write(&report, b"session-ready\n").unwrap();
                while std::fs::read_to_string(&report).unwrap() != "continue\n" {
                    assert!(Instant::now() < deadline);
                    std::thread::sleep(Duration::from_millis(1));
                }
                if session_frame {
                    let mut pixels = image.take().unwrap();
                    let capacity = pixels.capacity();
                    assert!(session.frame_buffer(&mut pixels, 0).is_err());
                    assert_eq!(pixels.as_ptr(), image_pointer);
                    assert_eq!(pixels.capacity(), capacity);
                    assert!(pixels.iter().all(|&byte| byte == 0xa5));
                    session.frame_buffer(&mut pixels, expires).unwrap();
                    assert!(pixels.is_empty() && pixels.capacity() == 0);
                } else if session_capture {
                    session
                        .capture(
                            sophia_portal_client::protocol::CaptureRequest {
                                serial: 2,
                                output: 7,
                            },
                            1,
                            expires,
                        )
                        .unwrap();
                } else {
                    session.transfer(buffer.take().unwrap(), expires).unwrap();
                }
                if mode == "sdk-session-cancel" {
                    session.cancel_watch().unwrap();
                }
                phase = 1;
            }
            if session_frame && !frame_cancelled && session.frame_received() > 0 {
                assert!(session.frame_result().is_none());
                session.cancel_watch().unwrap();
                frame_cancelled = true;
            }
            if phase == 1
                && session.watch_event().is_some()
                && (if session_frame {
                    session.frame_result().is_some()
                } else if session_capture {
                    session.capture_result().is_some()
                } else {
                    session.snapshot_result().is_some()
                })
            {
                break;
            }
        }
        assert_eq!(
            session.watch_event(),
            Some(if mode == "sdk-session-cancel" || session_frame {
                WatchEvent::Cancelled
            } else {
                WatchEvent::Revoked
            })
        );
        session.release_watch().unwrap();
        if session_frame {
            assert!(frame_cancelled);
            let (info, image) = session.take_frame().unwrap();
            assert_eq!(image.as_ptr(), image_pointer);
            assert_eq!(info.image_bytes, 256 * 128 * 4);
            assert!(image.iter().all(|b| *b == 17));
        } else if session_capture {
            assert_eq!(
                session.capture_result(),
                Some((sophia_portal_client::protocol::CaptureState::Ready, 2))
            );
            session.release_capture().unwrap();
        } else {
            let (buffer, length) = session.take_transfer().unwrap();
            assert_eq!(buffer.as_ptr(), pointer);
            assert_eq!(&buffer[..length], b"granted");
            assert!(buffer[length..].iter().all(|b| *b == 0xa5));
        }
        let role = session.finish().unwrap();
        assert_eq!(role.pipeline.fid_count(), 1);
        assert!(role.pipeline.is_quiescent());
        std::fs::write(
            std::env::var_os("SOPHIA_TEST_LOCK_REPORT").unwrap(),
            b"session-done\n",
        )
        .unwrap();
        return;
    }
    if mode.starts_with("sdk-watch") {
        let absent = mode == "sdk-watch-absent";
        let cancel = mode == "sdk-watch-cancel";
        let expire = mode == "sdk-watch-expire";
        let now = start.elapsed().as_millis() as u64;
        let mut watch = sophia_portal_client::watch::Watch::new(opened, now, now + 3000).unwrap();
        let mut announced = false;
        while !watch.revoked() && watch.error().is_none() {
            assert!(Instant::now() < deadline);
            let interest = watch.io_interest().unwrap();
            let fd = watch.fd().unwrap();
            let mut flags = rustix::event::PollFlags::IN;
            if interest.writable {
                flags |= rustix::event::PollFlags::OUT;
            }
            let mut fds = [rustix::event::PollFd::new(&fd, flags)];
            let wait = Duration::from_millis(if interest.immediate {
                0
            } else {
                watch
                    .deadline()
                    .unwrap()
                    .saturating_sub(start.elapsed().as_millis() as u64)
            });
            rustix::event::poll(
                &mut fds,
                Some(&rustix::event::Timespec::try_from(wait).unwrap()),
            )
            .unwrap();
            let result = watch.dispatch(start.elapsed().as_millis() as u64, 65536);
            if !absent {
                result.unwrap();
            }
            if !announced && watch.waiting() && !watch.io_interest().unwrap().writable {
                assert!(!watch.revoked());
                let report = std::env::var_os("SOPHIA_TEST_LOCK_REPORT").unwrap();
                std::fs::write(&report, b"watch-ready\n").unwrap();
                while std::fs::read_to_string(&report).unwrap() != "continue\n" {
                    assert!(Instant::now() < deadline);
                    std::thread::sleep(Duration::from_millis(1));
                }
                announced = true;
                if cancel {
                    watch.close();
                }
                if expire {
                    assert_eq!(
                        watch.dispatch(watch.deadline().unwrap(), 65536),
                        Err(sophia_portal_client::watch::Error::Deadline)
                    );
                }
            }
        }
        if absent || cancel || expire {
            let expected = if absent {
                sophia_portal_client::watch::Error::Remote(2)
            } else if cancel {
                sophia_portal_client::watch::Error::Closed
            } else {
                sophia_portal_client::watch::Error::Deadline
            };
            assert_eq!(watch.error(), Some(&expected));
            assert!(!watch.revoked() && watch.fd().is_none());
        } else {
            assert!(announced && watch.revoked());
            let role = watch.finish().unwrap();
            assert_eq!(role.pipeline.fid_count(), 1);
            assert!(role.pipeline.is_quiescent());
        }
        std::fs::write(
            std::env::var_os("SOPHIA_TEST_LOCK_REPORT").unwrap(),
            b"watch-done\n",
        )
        .unwrap();
        return;
    }
    if mode.starts_with("sdk-transfer") {
        let absent = mode == "sdk-transfer-absent";
        let revoked = mode == "sdk-transfer-revoke";
        let mut storage = vec![0xa5; 65536];
        let pointer = storage.as_ptr();
        let now = start.elapsed().as_millis() as u64;
        let mut transfer =
            sophia_portal_client::transfer::Transfer::new(opened, &mut storage, now, now + 3000)
                .unwrap();
        let mut paused = false;
        while transfer.result().is_none() && transfer.error().is_none() {
            assert!(Instant::now() < deadline);
            let interest = transfer.io_interest().unwrap();
            let fd = transfer.fd().unwrap();
            let mut flags = rustix::event::PollFlags::IN;
            if interest.writable {
                flags |= rustix::event::PollFlags::OUT;
            }
            let mut fds = [rustix::event::PollFd::new(&fd, flags)];
            let wait = Duration::from_millis(if interest.immediate {
                0
            } else {
                transfer
                    .deadline()
                    .unwrap()
                    .saturating_sub(start.elapsed().as_millis() as u64)
            });
            rustix::event::poll(
                &mut fds,
                Some(&rustix::event::Timespec::try_from(wait).unwrap()),
            )
            .unwrap();
            let result = transfer.dispatch(
                start.elapsed().as_millis() as u64,
                if revoked && !paused { 1 } else { 65536 },
            );
            if !absent && !revoked {
                result.unwrap();
            }
            if revoked && !paused && transfer.received() > 0 {
                assert!(transfer.result().is_none());
                let report = std::env::var_os("SOPHIA_TEST_LOCK_REPORT").unwrap();
                std::fs::write(&report, b"transfer-partial\n").unwrap();
                while std::fs::read_to_string(&report).unwrap() != "continue\n" {
                    assert!(Instant::now() < deadline);
                    std::thread::sleep(Duration::from_millis(1));
                }
                paused = true;
            }
        }
        if absent || revoked {
            assert!(transfer.result().is_none() && transfer.fd().is_none());
            assert_eq!(
                transfer.error(),
                Some(&sophia_portal_client::transfer::Error::Snapshot(
                    sophia_portal_client::snapshot::Error::Remote(if absent { 2 } else { 116 })
                ))
            );
            assert!(!revoked || paused);
        } else {
            let (role, bytes) = transfer.finish().unwrap();
            assert_eq!(bytes, b"granted");
            assert_eq!(bytes.as_ptr(), pointer);
            assert_eq!(role.pipeline.fid_count(), 1);
            assert!(role.pipeline.is_quiescent());
            assert!(storage[7..].iter().all(|b| *b == 0xa5));
        }
        std::fs::write(
            std::env::var_os("SOPHIA_TEST_LOCK_REPORT").unwrap(),
            b"transfer-done\n",
        )
        .unwrap();
        return;
    }
    if request_mode {
        let now = start.elapsed().as_millis() as u64;
        let mut request = sophia_portal_client::capture_request::Request::new(
            opened,
            sophia_portal_client::protocol::CaptureRequest {
                serial: if mode == "sdk-request-busy" { 2 } else { 1 },
                output: 7,
            },
            0,
            now,
            now + 3000,
        )
        .unwrap();
        while request.outcome().is_none() && request.error().is_none() {
            assert!(Instant::now() < deadline);
            let interest = request.io_interest().unwrap();
            let fd = request.fd().unwrap();
            let mut flags = rustix::event::PollFlags::IN;
            if interest.writable {
                flags |= rustix::event::PollFlags::OUT;
            }
            let mut fds = [rustix::event::PollFd::new(&fd, flags)];
            let wait = Duration::from_millis(if interest.immediate {
                0
            } else {
                request
                    .next_wake()
                    .unwrap()
                    .saturating_sub(start.elapsed().as_millis() as u64)
            });
            rustix::event::poll(
                &mut fds,
                Some(&rustix::event::Timespec::try_from(wait).unwrap()),
            )
            .unwrap();
            let result = request.dispatch(start.elapsed().as_millis() as u64, 65536);
            if !refused {
                result.unwrap();
            }
        }
        if refused {
            assert_eq!(
                request.custody(),
                sophia_portal_client::capture::Custody::Refused
            );
            assert_eq!(
                request.error(),
                Some(&sophia_portal_client::capture_request::Error::Capture(
                    sophia_portal_client::capture::Error::Refused(refusal_errno)
                ))
            );
            assert!(request.outcome().is_none() && request.fd().is_none());
            std::fs::write(
                std::env::var_os("SOPHIA_TEST_LOCK_REPORT").unwrap(),
                b"request-refused\n",
            )
            .unwrap();
            return;
        }
        let (role, outcome, serial) = request.finish().unwrap();
        assert_eq!(
            outcome,
            if cancelled {
                sophia_portal_client::protocol::CaptureState::Cancelled
            } else {
                sophia_portal_client::protocol::CaptureState::Ready
            }
        );
        assert_eq!(serial, 1);
        assert_eq!(role.pipeline.fid_count(), 1);
        if cancelled {
            assert!(role.pipeline.is_quiescent());
            std::fs::write(
                std::env::var_os("SOPHIA_TEST_LOCK_REPORT").unwrap(),
                b"request-cancelled\n",
            )
            .unwrap();
            return;
        }
        opened = role;
    }
    if frame_mode {
        let now = start.elapsed().as_millis() as u64;
        let mut frame = sophia_portal_client::frame::Frame::new(
            opened,
            Vec::with_capacity(256 * 128 * 4),
            now,
            now + 3000,
        )
        .unwrap();
        let mut paused = false;
        while frame.result().is_none() && frame.error().is_none() {
            assert!(Instant::now() < deadline);
            let interest = frame.io_interest().unwrap();
            let fd = frame.fd().unwrap();
            let mut flags = rustix::event::PollFlags::IN;
            if interest.writable {
                flags |= rustix::event::PollFlags::OUT;
            }
            let mut fds = [rustix::event::PollFd::new(&fd, flags)];
            let wait = Duration::from_millis(if interest.immediate {
                0
            } else {
                frame
                    .deadline()
                    .unwrap()
                    .saturating_sub(start.elapsed().as_millis() as u64)
            });
            rustix::event::poll(
                &mut fds,
                Some(&rustix::event::Timespec::try_from(wait).unwrap()),
            )
            .unwrap();
            let result = frame.dispatch(
                start.elapsed().as_millis() as u64,
                if revoke && !paused { 1 } else { 65536 },
            );
            if !revoke {
                result.unwrap();
            }
            if revoke && !paused && frame.received() > 0 {
                assert!(frame.received() < 256 * 128 * 4 && frame.result().is_none());
                let report = std::env::var_os("SOPHIA_TEST_LOCK_REPORT").unwrap();
                std::fs::write(&report, b"partial\n").unwrap();
                while std::fs::read_to_string(&report).unwrap() != "continue\n" {
                    assert!(Instant::now() < deadline);
                    std::thread::sleep(Duration::from_millis(1));
                }
                paused = true;
            }
        }
        if revoke {
            assert!(paused && frame.result().is_none() && frame.fd().is_none());
            assert!(!matches!(
                frame.error(),
                Some(sophia_portal_client::frame::Error::Deadline)
            ));
            std::fs::write(
                std::env::var_os("SOPHIA_TEST_LOCK_REPORT").unwrap(),
                b"revoked\n",
            )
            .unwrap();
            return;
        }
        let (opened, info, pixels) = frame.finish().unwrap();
        assert_eq!(
            (
                info.output,
                info.output_generation,
                info.presentation_generation
            ),
            (7, 9, 11)
        );
        assert_eq!((info.width, info.height), (256, 128));
        assert_eq!(pixels.len(), 256 * 128 * 4);
        assert!(pixels.iter().all(|b| *b == 17));
        assert_eq!(opened.pipeline.fid_count(), 1);
    }
    std::fs::write(
        std::env::var_os("SOPHIA_TEST_LOCK_REPORT").unwrap(),
        if frame_mode {
            b"frame\n".as_slice()
        } else {
            b"started\n"
        },
    )
    .unwrap();
}

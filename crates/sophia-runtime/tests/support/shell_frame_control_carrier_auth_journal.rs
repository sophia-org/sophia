//! Journal operations remain socket-framed in both carrier treatments.
use super::*;

fn open(fixture: &mut Fixture, name: &[u8], mode: u32) -> Fid {
    let (_, fid) = fixture.client.walk(fixture.root, &[name]).unwrap();
    assert!(matches!(
        reply(&mut fixture.wire, &mut fixture.client),
        Reply::Walk(_)
    ));
    fixture.client.lopen(fid, mode).unwrap();
    assert!(matches!(
        reply(&mut fixture.wire, &mut fixture.client),
        Reply::Lopen { .. }
    ));
    fid
}

fn submit(fixture: &mut Fixture, carrier: &mut Carrier, bytes: &[u8]) -> Receipt {
    carrier.capture(bytes);
    let (input, output) = wake::pipe().unwrap();
    fixture
        .binding
        .process(&mut fixture.wire, &mut carrier.slot, &carrier.captured)
        .unwrap();
    fixture
        .binding
        .deliver(&mut fixture.wire, &mut carrier.slot, &output)
        .unwrap();
    input.receipt().unwrap().unwrap()
}

#[test]
fn socket_events_and_idle_ack_preserve_carrier_publication_hold() {
    let mut control = Fixture::new();
    let mut candidate = Fixture::new();
    let mut carrier = Carrier::for_writer(candidate.binding.writer);
    let bytes = commit(20, 0);
    assert_eq!(control.write(&bytes), Reply::Write(bytes.len() as u32));
    agrees(
        Ok(bytes.len() as u32),
        submit(&mut candidate, &mut carrier, &bytes),
        &bytes,
    );
    let mut observed = Vec::new();
    for fixture in [&mut control, &mut candidate] {
        let events = open(fixture, b"events", 0);
        let ack = open(fixture, b"ack", 1);
        let files = fixture.wire.export_mut();
        files
            .with_frame_commit(|input| {
                input.take();
                Ok(crate::ContentFrameAdmission::Accepted { copy_job: None })
            })
            .unwrap();
        assert!(!files.publish_image_records());
        files
            .append_event(ShellFileKind::CandidateOutcome, &[0; 40], false)
            .unwrap();
        assert!(!files.publish_image_records());
        let sequence = files
            .append_event(ShellFileKind::FrameOutcome, &[0; 116], false)
            .unwrap();
        assert!(files.publish_image_records());
        let expected = journal_bytes(files);
        assert!(!expected.is_empty());
        fixture.client.read(events, 0, 8192).unwrap();
        assert_eq!(
            reply(&mut fixture.wire, &mut fixture.client),
            Reply::Read(expected.clone())
        );
        let bytes = encode_shell_file_ack(ShellFileAck {
            connection_epoch: 1,
            sequence,
        })
        .unwrap();
        fixture.client.write(ack, 0, &bytes).unwrap();
        assert_eq!(
            reply(&mut fixture.wire, &mut fixture.client),
            Reply::Write(bytes.len() as u32)
        );
        fixture.client.read(events, 0, 8192).unwrap();
        assert_eq!(
            reply(&mut fixture.wire, &mut fixture.client),
            Reply::Error(Errno::ESTALE)
        );
        assert_eq!(
            fixture.wire.export().submission_watermark,
            20,
            "idle ACK is not a commit"
        );
        observed.push(expected);
    }
    assert_eq!(observed[0], observed[1]);
    equal(control.wire.export(), candidate.wire.export());
}

#[test]
fn socket_and_carrier_apply_ack_before_backpressure_with_a_full_journal() {
    let mut control = Fixture::new();
    let mut candidate = Fixture::new();
    let mut carrier = Carrier::for_writer(candidate.binding.writer);
    let mut sequences = Vec::new();
    for fixture in [&mut control, &mut candidate] {
        let files = fixture.wire.export_mut();
        let mut last = 0;
        let mut full = false;
        for _ in 0..=usize::from(sophia_protocol::shell_files::SHELL_FILE_MAX_JOURNAL_RECORDS) {
            match files.append_event(ShellFileKind::FrameOutcome, &[0; 116], true) {
                Ok(sequence) => last = sequence,
                Err(_) => {
                    full = true;
                    break;
                }
            }
        }
        assert!(full && last > 0, "bounded fixture must fill the journal");
        sequences.push(last);
    }
    assert_eq!(sequences[0], sequences[1]);
    let before = journal_bytes(control.wire.export());
    let first = commit(20, 0);
    assert_eq!(control.write(&first), Reply::Write(first.len() as u32));
    agrees(
        Ok(first.len() as u32),
        submit(&mut candidate, &mut carrier, &first),
        &first,
    );
    assert_eq!(
        journal_bytes(candidate.wire.export()),
        before,
        "receipt spends no journal capacity"
    );
    equal(control.wire.export(), candidate.wire.export());

    let bytes = commit(21, sequences[0]);
    let value = decode_shell_frame_record(&bytes, ShellFileClass::Candidate, 13, 8192).unwrap();
    for fixture in [&mut control, &mut candidate] {
        let files = fixture.wire.export_mut();
        while files.inbound.len() < INBOUND_RECORDS {
            files
                .inbound
                .push_back(Inbound::Frame(Box::new(value.clone())));
        }
    }
    assert_eq!(control.write(&bytes), Reply::Error(Errno::EAGAIN));
    let receipt = submit(&mut candidate, &mut carrier, &bytes);
    agrees(Err(Errno::EAGAIN), receipt, &bytes);
    let mut client = Client::default();
    client.begin(receipt.identity).unwrap();
    publish_client(&mut client);
    client.observe(receipt).unwrap();
    assert_eq!(
        client.confirmed_ack, 0,
        "refusal does not confirm applied ACK"
    );
    for fixture in [&mut control, &mut candidate] {
        assert!(
            matches!(
                fixture.wire.export().journal.read(0, 8192),
                Err(Errno::ESTALE)
            ),
            "ACK applied before admission refused"
        );
        assert_eq!(fixture.wire.export().submission_watermark, 20);
        fixture.wire.export_mut().inbound.clear();
    }
    equal(control.wire.export(), candidate.wire.export());
    assert_eq!(control.write(&bytes), Reply::Write(bytes.len() as u32));
    let receipt = submit(&mut candidate, &mut carrier, &bytes);
    agrees(Ok(bytes.len() as u32), receipt, &bytes);
    client.begin(receipt.identity).unwrap();
    publish_client(&mut client);
    client.observe(receipt).unwrap();
    assert_eq!(client.confirmed_ack, sequences[0]);
    equal(control.wire.export(), candidate.wire.export());
    assert_eq!(
        control.wire.export_mut().take_inbound(),
        candidate.wire.export_mut().take_inbound()
    );
}

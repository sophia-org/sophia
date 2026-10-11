use super::*;

const AUTH: Qid = Qid {
    kind: QidKind::Auth,
    version: 0,
    path: 3,
};
const AUTH_BYTES: [u8; 13] = [8, 0, 0, 0, 0, 3, 0, 0, 0, 0, 0, 0, 0];

#[test]
fn auth_conversation_and_attach_carry_the_exact_afid_and_claims() {
    let (client, thread) = scripted(
        |stream| {
            versioned(stream);
            let (kind, tag, body) = request(stream);
            assert_eq!(kind, 102);
            let afid = u32::from_le_bytes(body[..4].try_into().unwrap());
            let mut expected = afid.to_le_bytes().to_vec();
            string(&mut expected, b"user");
            string(&mut expected, b"view");
            expected.extend_from_slice(&u32::MAX.to_le_bytes());
            assert_eq!(body, expected);
            stream.write_all(&reply(103, tag, &AUTH_BYTES)).unwrap();
            let (kind, tag, body) = request(stream);
            assert_eq!(kind, 116);
            assert_eq!(&body[..4], &afid.to_le_bytes());
            stream.write_all(&rread(tag, b"challenge")).unwrap();
            let (kind, tag, body) = request(stream);
            assert_eq!(kind, 104);
            assert_eq!(&body[4..8], &afid.to_le_bytes());
            assert_eq!(&body[8..], &expected[4..]);
            stream.write_all(&reply(105, tag, &QID_DIR)).unwrap();
            let (kind, tag, body) = request(stream);
            assert_eq!(kind, 120);
            assert_eq!(body, afid.to_le_bytes());
            stream.write_all(&reply(121, tag, &[])).unwrap();
            hold(stream);
        },
        PipelineLimits::default(),
    );
    let mut p = client.unwrap();
    let (tag, afid) = p.auth(b"user", b"view").unwrap();
    assert_eq!(p.wait(tag, deadline()).unwrap(), Reply::Auth(AUTH));
    let tag = p.read(afid, 0, 32).unwrap();
    assert_eq!(
        p.wait(tag, deadline()).unwrap(),
        Reply::Read(b"challenge".to_vec())
    );
    let (tag, root) = p.attach_with_afid(afid, b"user", b"view").unwrap();
    assert_ne!(afid, root);
    assert_eq!(p.wait(tag, deadline()).unwrap(), Reply::Attach(DIR));
    assert_eq!(p.fid_count(), 2);
    let tag = p.clunk(afid).unwrap();
    assert_eq!(p.wait(tag, deadline()).unwrap(), Reply::Clunk);
    assert_eq!(p.fid_count(), 1);
    drop(p);
    thread.join().unwrap();
}

#[test]
fn auth_flush_preserves_only_an_already_answered_afid() {
    for answered in [false, true] {
        let (client, thread) = scripted(
            move |s| {
                versioned(s);
                let (_, a, _) = request(s);
                let (_, f, _) = request(s);
                if answered {
                    s.write_all(&reply(103, a, &AUTH_BYTES)).unwrap();
                }
                s.write_all(&reply(109, f, &[])).unwrap();
                hold(s);
            },
            PipelineLimits::default(),
        );
        let mut p = client.unwrap();
        let (a, _) = p.auth(b"", b"").unwrap();
        let f = p.flush(a).unwrap();
        assert_eq!(p.wait(f, deadline()).unwrap(), Reply::Flush);
        assert_eq!(
            p.fid_count(),
            usize::from(answered),
            "only an answered auth survives flush"
        );
        assert_eq!(p.take_reply(), answered.then_some((a, Reply::Auth(AUTH))));
        drop(p);
        thread.join().unwrap();
    }
}

#[test]
fn refused_auth_releases_its_fid_and_wrong_qid_types_poison() {
    let (client, thread) = scripted(
        |s| {
            versioned(s);
            let (_, tag, _) = request(s);
            s.write_all(&rlerror(tag, 13)).unwrap();
            hold(s);
        },
        PipelineLimits::default(),
    );
    let mut p = client.unwrap();
    let (tag, _) = p.auth(b"", b"").unwrap();
    assert_eq!(
        p.wait(tag, deadline()).unwrap(),
        Reply::Error(Errno::EACCES)
    );
    assert_eq!(p.fid_count(), 0);
    drop(p);
    thread.join().unwrap();
    for bytes in [
        QID_DIR.to_vec(),
        QID_FILE.to_vec(),
        AUTH_BYTES[..12].to_vec(),
        [AUTH_BYTES.as_slice(), &[0]].concat(),
    ] {
        let (client, thread) = scripted(
            move |s| {
                versioned(s);
                let (_, tag, _) = request(s);
                s.write_all(&reply(103, tag, &bytes)).unwrap();
                hold(s);
            },
            PipelineLimits::default(),
        );
        let mut p = client.unwrap();
        let (tag, _) = p.auth(b"", b"").unwrap();
        assert_eq!(
            p.wait(tag, deadline()),
            Err(PipelineError::Protocol("Rauth shape"))
        );
        drop(p);
        thread.join().unwrap();
    }
}

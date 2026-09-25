//! 9P2000 binary wire encoding for responses (R-messages).

use crate::types::{Qid, RMessage, Tag};

pub fn encode_string(buf: &mut Vec<u8>, s: &str) {
    let len = s.len() as u16;
    buf.extend_from_slice(&len.to_le_bytes());
    buf.extend_from_slice(s.as_bytes());
}

pub fn encode_qid(buf: &mut Vec<u8>, qid: &Qid) {
    buf.push(qid.qtype);
    buf.extend_from_slice(&qid.version.to_le_bytes());
    buf.extend_from_slice(&qid.path.to_le_bytes());
}

pub fn encode_r_message(tag: Tag, msg: &RMessage) -> Vec<u8> {
    let mut payload = Vec::new();

    let msg_type: u8 = match msg {
        RMessage::Version { msize, version } => {
            payload.extend_from_slice(&msize.to_le_bytes());
            encode_string(&mut payload, version);
            101
        }
        RMessage::Auth { aqid } => {
            encode_qid(&mut payload, aqid);
            103
        }
        RMessage::Attach { qid } => {
            encode_qid(&mut payload, qid);
            105
        }
        RMessage::Error { ename } => {
            encode_string(&mut payload, ename);
            107
        }
        RMessage::Flush => 109,
        RMessage::Walk { wqids } => {
            let nwqid = wqids.len() as u16;
            payload.extend_from_slice(&nwqid.to_le_bytes());
            for qid in wqids {
                encode_qid(&mut payload, qid);
            }
            111
        }
        RMessage::Open { qid, iounit } => {
            encode_qid(&mut payload, qid);
            payload.extend_from_slice(&iounit.to_le_bytes());
            113
        }
        RMessage::Create { qid, iounit } => {
            encode_qid(&mut payload, qid);
            payload.extend_from_slice(&iounit.to_le_bytes());
            115
        }
        RMessage::Read { data } => {
            let count = data.len() as u32;
            payload.extend_from_slice(&count.to_le_bytes());
            payload.extend_from_slice(data);
            117
        }
        RMessage::Write { count } => {
            payload.extend_from_slice(&count.to_le_bytes());
            119
        }
        RMessage::Clunk => 121,
        RMessage::Remove => 123,
        RMessage::Stat { stat_bytes } => {
            let len = stat_bytes.len() as u16;
            payload.extend_from_slice(&len.to_le_bytes());
            payload.extend_from_slice(stat_bytes);
            125
        }
        RMessage::Wstat => 127,
    };

    let total_size = (4 + 1 + 2 + payload.len()) as u32;
    let mut out = Vec::with_capacity(total_size as usize);
    out.extend_from_slice(&total_size.to_le_bytes());
    out.push(msg_type);
    out.extend_from_slice(&tag.raw().to_le_bytes());
    out.extend_from_slice(&payload);
    out
}

use std::{collections::BTreeMap, io::Write};

pub fn finish(files: bool, probe: bool, frames: u64) {
    let events = sophia_9p::wire_trace::take();
    let mut paths = BTreeMap::<(u64, u32), String>::new();
    let mut pending = BTreeMap::<(u64, u16), (usize, String, &'static str, usize)>::new();
    let mut last_record = None;
    let mut pairs = Vec::new();
    let mut reads = Vec::new();
    let mut trace = Vec::new();
    for (index, e) in events.into_iter().enumerate() {
        let key = (e.connection, e.tag);
        let path = paths
            .get(&(e.connection, e.fid))
            .cloned()
            .unwrap_or_default();
        trace.push(serde_json::json!({"index":index,"connection":e.connection,
            "phase":e.phase,"tag":e.tag,"op":e.op,"fid":e.fid,"newfid":e.newfid,
            "offset":e.offset,"count":e.count,"names":e.names,"path":path}));
        match e.phase {
            "request" => {
                if e.op == "attach" {
                    paths.insert((e.connection, e.fid), String::new());
                }
                if e.op == "walk" {
                    let joined = if path.is_empty() {
                        e.names.clone()
                    } else {
                        format!("{path}/{}", e.names)
                    };
                    paths.insert((e.connection, e.newfid), joined);
                }
                if e.op == "write" && (path == "transaction" || path == "transaction-reuse") {
                    last_record = Some((key, index));
                }
                if e.op == "write" && path == "submit" {
                    let (record_key, record_index) =
                        last_record.take().expect("submit follows a record");
                    let overlap = pending
                        .get(&record_key)
                        .is_some_and(|p| p.0 == record_index);
                    pairs.push(serde_json::json!({"record_request":record_index,"submit_request":index,
                        "record_tag":record_key.1,"submit_tag":e.tag,"record_reply_not_sent":overlap}));
                }
                let read_index = reads.len();
                if e.op == "read" {
                    reads.push(serde_json::json!({"request":index,"tag":e.tag,"path":path,
                        "offset":e.offset,"requested":e.count,"reply":null,"sent":false}));
                }
                assert!(
                    pending
                        .insert(key, (index, path, e.op, read_index))
                        .is_none(),
                    "live tag reuse"
                );
            }
            "reply" => {
                let p = pending.get(&key).expect("reply has a request");
                if p.2 == "read" {
                    reads[p.3]["reply"] =
                        serde_json::json!({"index":index,"op":e.op,"count":e.count});
                }
            }
            "sent" => {
                let p = pending.remove(&key).expect("sent reply has a request");
                if p.2 == "read" {
                    reads[p.3]["sent"] = true.into();
                }
            }
            _ => panic!("unknown trace phase"),
        }
    }
    let overlaps = pairs
        .iter()
        .filter(|p| p["record_reply_not_sent"] == true)
        .count();
    if files {
        assert_eq!(pairs.len(), frames as usize * if probe { 8 } else { 5 } + 1);
        // Ordinary negotiation stays sequential. Every steady transaction pairs.
        assert_eq!(overlaps, pairs.len() - 1);
    } else {
        assert!(trace.is_empty());
    }
    if let Some(directory) = std::env::var_os("SOPHIA_TIMING_PEER_JOURNALS") {
        let stem = format!(
            "{}{}",
            if files { "files" } else { "ipc" },
            if probe { "-4mib" } else { "" }
        );
        let directory = std::path::Path::new(&directory);
        let mut raw = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(directory.join(format!("{stem}.wire-trace.ndjson")))
            .unwrap();
        for event in trace {
            writeln!(raw, "{event}").unwrap();
        }
        let summary = serde_json::json!({"scope":"buffered shell request dispatch and complete reply send; diagnostic only",
            "frames":frames,"submits":pairs.len(),"overlaps":overlaps,"pairs":pairs,"reads":reads});
        serde_json::to_writer_pretty(
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(directory.join(format!("{stem}.wire-summary.json")))
                .unwrap(),
            &summary,
        )
        .unwrap();
    }
}

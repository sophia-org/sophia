//! Socket (`sophia_wm_v1`) frames of the neutral record values, compared with
//! the golden captured before extraction. IPC-only: it retires with the
//! socket wire (t269). Includers name the neutral values module `fixture`.
use super::fixture::*;
use sophia_protocol::*;

pub fn legacy_bytes() -> Vec<u8> {
    let mut bytes = Vec::new();
    for capabilities in [0, u64::MAX] {
        let mut snapshot = encode_wm_v1_policy_snapshot(
            TransactionId::from_raw(9),
            2,
            &scene(),
            &actions(),
            &classifications(),
            capabilities,
        )
        .unwrap();
        append_wm_launch_origins(&mut snapshot, &origins(), capabilities).unwrap();
        bytes.extend(
            encode_wm_v1_snapshot_begin_frame(snapshot.transaction, &snapshot.begin).unwrap(),
        );
        for c in &snapshot.chunks {
            bytes.extend(encode_wm_v1_snapshot_chunk_frame(snapshot.transaction, c).unwrap());
        }
        bytes.extend(encode_wm_v1_snapshot_end_frame(snapshot.transaction, &snapshot.end).unwrap());
    }
    for large in [false, true] {
        let mut p = proposal();
        if large {
            let scene = p.presentation.as_mut().unwrap();
            let instance = scene.instances[0];
            scene.instances = (0..1024)
                .map(|i| PolicySurfaceInstance {
                    id: i + 2,
                    z_index: i as u16 + 1,
                    ..instance
                })
                .collect();
        }
        let projection = encode_wm_v1_policy_projection(&p).unwrap();
        bytes.extend(
            encode_wm_v1_projection_begin_frame(projection.transaction, &projection.begin).unwrap(),
        );
        for c in &projection.chunks {
            bytes.extend(encode_wm_v1_projection_chunk_frame(projection.transaction, c).unwrap());
        }
        bytes.extend(
            encode_wm_v1_projection_end_frame(projection.transaction, &projection.end).unwrap(),
        );
    }
    bytes
}

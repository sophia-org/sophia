//! Production borrowed WM admission -> real WM transport -> independent Hagia
//! decoder/reducer -> Engine stage/commit. No display, native input or GPU.
use super::*;
use crate::live_session::{LiveIndicatorAdmission, LiveWmRequestAdmission};
use sophia_runtime::{PolicyClientEvent, PolicyWmSessionTransport, QueuedPolicyProjection};
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Stdio};

struct Child(std::process::Child);
impl Drop for Child {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
#[ignore = "requires an explicit freshly built independent Hagia binary"]
fn two_output_click_queue_reaches_hagia_without_active_output_retargeting() {
    let binary = std::env::var_os("SOPHIA_HAGIA_BIN").expect("provide the tested Hagia binary");
    let directory = std::env::temp_dir().join(format!("targeted-hagia-{}", std::process::id()));
    let mut transport = PolicyWmSessionTransport::bind_for_supervised_uid(
        &directory,
        rustix::process::geteuid().as_raw(),
    )
    .unwrap();
    let profile = directory.join("desktop.kdl");
    std::fs::write(&profile, "schema 1\npolicy { view-count 3; workspace 1 output-key=1; workspace 2 output-key=1; workspace 3 output-key=1; workspace 4 output-key=2; workspace 5 output-key=2; workspace 6 output-key=2; }\n").unwrap();
    std::fs::set_permissions(&profile, std::fs::Permissions::from_mode(0o600)).unwrap();
    let mut command = Command::new(binary);
    for (name, _) in std::env::vars_os() {
        if name.to_string_lossy().starts_with("SOPHIA_")
            || name.to_string_lossy().starts_with("HAGIA_")
        {
            command.env_remove(name);
        }
    }
    let child = Child(
        command
            .arg(format!("--config={}", profile.display()))
            .arg(format!("--socket={}", transport.socket_path().display()))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .spawn()
            .unwrap(),
    );
    transport.authorize_supervised_pid(child.0.id()).unwrap();
    transport
        .accept_and_negotiate(1, Duration::from_secs(4))
        .unwrap();
    assert_ne!(
        transport.selected_capabilities() & SOPHIA_WM_CAPABILITY_OUTPUT_ACTIONS,
        0
    );
    let PolicyClientEvent::Configuration {
        transaction,
        configuration,
    } = transport
        .receive_client_event_within(Duration::from_secs(4))
        .unwrap()
    else {
        panic!("configuration");
    };
    transport
        .send_configuration_outcome(
            transaction,
            configuration.generation,
            PolicyProjectionOutcome::Committed,
        )
        .unwrap();
    let left = OutputId::from_raw(1);
    let right = OutputId::from_raw(2);
    let outputs = [left, right].map(|id| sophia_engine::HeadlessOutput {
        id,
        size: Size {
            width: 1000,
            height: 700,
        },
        scale: 1,
    });
    let scene = PolicySceneSnapshot {
        generation: 1,
        active_output: left,
        outputs: outputs
            .iter()
            .map(|o| {
                let bounds = Rect {
                    x: if o.id == left { 0 } else { 1000 },
                    y: 0,
                    width: 1000,
                    height: 700,
                };
                PolicyOutputSnapshot {
                    output: o.id,
                    generation: 1,
                    policy_key: Some(o.id.raw()),
                    focus: None,
                    bounds,
                    work_area: bounds,
                }
            })
            .collect(),
        surfaces: vec![],
        session_operations: vec![],
    };
    let mut reducer = sophia_engine::PolicyProjectionReducer::new(scene).unwrap();
    reducer.connect(1).unwrap();
    let generations = [(left, 1), (right, 1)].into_iter().collect();
    let mut serial = 100;
    for (index, number) in [0, 5, 2, 6, 3].into_iter().enumerate() {
        let cause = if number == 0 {
            PolicyRequestCause::SceneChanged
        } else {
            let publication = reducer.indicator_publication();
            let target = if number <= 3 { left } else { right };
            let mut queue = std::collections::VecDeque::new();
            let result = LiveIndicatorAdmission {
                policy_connection_epoch: 1,
                publication: &publication,
                outputs: &outputs,
                output_generations: &generations,
                capabilities: transport.selected_capabilities(),
                next_transaction: &mut serial,
                queue: &mut queue,
                in_flight_source: None,
                in_flight: false,
            }
            .enqueue(WmActionId::from_raw(10 + number), target)
            .unwrap();
            assert_eq!(result.admission, LiveWmRequestAdmission::Admitted);
            assert_eq!(queue.len(), 1);
            queue.pop_front().unwrap().cause
        };
        let request = reducer
            .issue_request_with_cause(vec![left, right], cause)
            .unwrap();
        let snapshot = encode_wm_v1_policy_snapshot(
            TransactionId::from_raw(10 + index as u64 * 2),
            1,
            reducer.scene(),
            &[],
            &[],
            transport.selected_capabilities(),
        )
        .unwrap();
        transport
            .send_snapshot(
                snapshot.transaction,
                &snapshot.begin,
                &snapshot.chunks,
                &snapshot.end,
            )
            .unwrap();
        transport
            .send_projection_request(TransactionId::from_raw(11 + index as u64 * 2), &request)
            .unwrap();
        let proposal = loop {
            match transport
                .receive_client_event_within(Duration::from_secs(4))
                .unwrap()
            {
                PolicyClientEvent::Projection(QueuedPolicyProjection::Admitted(transfer)) => {
                    break decode_wm_v1_policy_projection(&transfer.into_wire_transfer()).unwrap();
                }
                PolicyClientEvent::ProjectionPending => {}
                _ => panic!("projection"),
            }
        };
        assert_eq!(
            proposal.active_output,
            if number > 3 { right } else { left }
        );
        for (output, numbers) in [(left, vec![11, 12, 13]), (right, vec![14, 15, 16])] {
            assert_eq!(
                proposal
                    .indicators
                    .iter()
                    .filter(|i| i.output == output)
                    .map(|i| i.action.unwrap().raw())
                    .collect::<Vec<_>>(),
                numbers
            );
        }
        let staged = reducer.stage_proposal(&proposal).unwrap();
        assert_eq!(
            reducer.commit_staged(staged),
            PolicyProjectionOutcome::Committed
        );
        transport
            .send_projection_outcome(
                proposal.transaction,
                request.request_id,
                reducer.scene().generation,
                PolicyProjectionOutcome::Committed,
            )
            .unwrap();
    }
    transport.disconnect().unwrap();
    drop(child);
    std::fs::remove_dir_all(directory).unwrap();
}

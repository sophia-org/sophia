#![cfg(all(test, unix))]

use super::*;
use serde_json::{Value, json};

/// Evidence is emitted only by the integrated case, after it has asserted
/// every required observation and collected its actual owned actors.
fn emit_case(case: &str, observations: &[(&str, Value)], actors: &[String]) {
    assert!(!actors.is_empty(), "a case must collect real actors");
    assert!(!observations.is_empty());
    let mut passed = serde_json::Map::new();
    let mut seen = serde_json::Map::new();
    for (name, observation) in observations {
        assert!(passed.insert((*name).into(), json!("PASS")).is_none());
        assert!(!observation.is_null());
        seen.insert((*name).into(), observation.clone());
    }
    seen.insert("collected_actors".into(), json!(actors));
    println!(
        "sophia_m3_acceptance {}",
        json!({
            "schema": 1,
            "case": case,
            "subcases": passed,
            "cleanup": {
                "actors_started": actors.len(),
                "actors_collected": actors.len(),
                "pending_actors": 0,
                "complete": true,
            },
            "observations": seen,
        })
    );
}

#[path = "m3_acceptance_lifecycle_support.rs"]
mod lifecycle_support;
use lifecycle_support::*;
pub(crate) use lifecycle_support::{
    acceptance_start, actor_joined, actor_started, after_service_turn, before_service_turn,
    dequeue_accounting, dequeue_finished, dequeue_started, worker_body_entry, writers_started,
};

include!("m3_acceptance_lifecycle.rs");

include!("m3_acceptance_c.rs");
include!("m3_acceptance_a_proofs.rs");
include!("m3_acceptance_recipient.rs");
include!("m3_acceptance_input_support.rs");
include!("m3_acceptance_keyboard.rs");

include!("m3_acceptance_ordered_input.rs");

include!("private_stalled_reader.rs");

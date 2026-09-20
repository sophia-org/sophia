//! Evidence for the M5 gate. Each group test starts real private Session
//! instances, accounts for every actor they started, and prints one record
//! the gate parses; the record's shape is what `cargo xtask check
//! m5-acceptance` requires, so it is pinned here and in the gate's own tests.

use sophia_session::private_input::{PrivateInputOutcome, PrivateInputThreadJoin};

#[derive(Default)]
pub struct Evidence {
    started: usize,
    collected: usize,
    invocations: usize,
}

impl Evidence {
    /// Account for one finished Session invocation: its service thread and
    /// every registered worker must have been joined. Used by each group test
    /// as it lands; until the first does, nothing in this target calls it.
    #[allow(dead_code)]
    pub fn collect(&mut self, outcome: PrivateInputOutcome, failed_setup: bool) {
        assert_eq!(outcome.service_thread, PrivateInputThreadJoin::Joined);
        if !failed_setup {
            assert!(outcome.failure.is_none(), "{outcome:?}");
        }
        let workers = outcome
            .workers
            .as_ref()
            .expect("ordinary invocation returns collection evidence");
        assert!(workers.iter().all(|worker| worker.joined), "{outcome:?}");
        assert!(!outcome.interrupted, "{outcome:?}");
        self.started += 1 + workers.len();
        self.collected += 1 + workers.iter().filter(|worker| worker.joined).count();
        self.invocations += 1;
    }

    /// Print the group's record. Every named subcase is reported passed: a
    /// subcase that did not hold has already failed the test by assertion.
    pub fn emit(self, group: &str, subcases: &[&str]) {
        assert_eq!(self.started, self.collected);
        assert!(self.invocations > 0);
        println!(
            "{}",
            record(
                group,
                subcases,
                self.started,
                self.collected,
                self.invocations
            )
        );
    }
}

/// The record line, as the gate reads it.
pub fn record(
    group: &str,
    subcases: &[&str],
    started: usize,
    collected: usize,
    invocations: usize,
) -> String {
    let subcases = subcases
        .iter()
        .map(|case| format!("\"{case}\":\"PASS\""))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "sophia_m5_acceptance {{\"schema\":1,\"case\":\"M5.{group}\",\"subcases\":{{{subcases}}},\"cleanup\":{{\"actors_started\":{started},\"actors_collected\":{collected},\"pending_actors\":0,\"complete\":true}},\"observations\":{{\"real_session_invocations\":{invocations},\"actor_scope\":\"service_threads_and_registered_ordered_workers\"}}}}"
    )
}

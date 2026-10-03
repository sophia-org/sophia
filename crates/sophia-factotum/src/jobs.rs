//! Slow work off the 9P server's thread: a small, bounded pool of workers
//! that runs PAM attempts and posts their outcomes back.
//!
//! The server is single-threaded. A conversation whose step needs a job
//! answers its read `Pending`; a worker runs the job, sends the outcome and
//! wakes the server, whose retry of the waiting read then finds it.

use crate::pam_helper::PamHelper;
use crate::proto::{Job, JobOutcome};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, SyncSender, TrySendError, channel, sync_channel};
use std::sync::{Arc, Mutex};

/// Identifies the conversation a job serves.
pub type ConversationId = u64;

/// Where the export hands jobs. The agent's pool implements it; tests
/// substitute their own.
pub trait JobSink {
    /// Queues a job; `false` if no worker can take it now, which the caller
    /// answers as an unavailable verdict.
    fn submit(&mut self, conversation: ConversationId, job: Job) -> bool;
    /// The conversation went away: stop its job and drop its outcome.
    fn cancel(&mut self, conversation: ConversationId);
}

struct Queued {
    conversation: ConversationId,
    job: Job,
    cancelled: Arc<AtomicBool>,
}

/// A fixed set of workers behind a bounded queue.
pub struct JobPool {
    queue: SyncSender<Queued>,
    cancels: HashMap<ConversationId, Arc<AtomicBool>>,
    _workers: Vec<std::thread::JoinHandle<()>>,
}

impl JobPool {
    /// Starts `workers` threads. Outcomes arrive on the returned receiver,
    /// and `wake` is called after each is sent.
    pub fn start(
        workers: usize,
        helper: PamHelper,
        wake: impl Fn() + Send + Sync + 'static,
    ) -> (Self, Receiver<(ConversationId, JobOutcome)>) {
        let (queue, jobs) = sync_channel::<Queued>(workers);
        let (outcomes, receiver) = channel();
        let jobs = Arc::new(Mutex::new(jobs));
        let wake = Arc::new(wake);
        let handles = (0..workers.max(1))
            .map(|_| {
                let jobs = Arc::clone(&jobs);
                let outcomes: Sender<(ConversationId, JobOutcome)> = outcomes.clone();
                let wake = Arc::clone(&wake);
                let helper = helper.clone();
                std::thread::spawn(move || {
                    loop {
                        let next = match jobs.lock() {
                            Ok(jobs) => jobs.recv(),
                            Err(_) => return,
                        };
                        let Ok(queued) = next else {
                            return;
                        };
                        let outcome = match &queued.job {
                            Job::Pam(request) => {
                                JobOutcome::Pam(helper.verify(request, &queued.cancelled))
                            }
                        };
                        drop(queued.job);
                        if queued.cancelled.load(Ordering::Acquire) {
                            continue;
                        }
                        if outcomes.send((queued.conversation, outcome)).is_err() {
                            return;
                        }
                        wake();
                    }
                })
            })
            .collect();
        (
            Self {
                queue,
                cancels: HashMap::new(),
                _workers: handles,
            },
            receiver,
        )
    }
}

impl JobSink for JobPool {
    fn submit(&mut self, conversation: ConversationId, job: Job) -> bool {
        let cancelled = Arc::new(AtomicBool::new(false));
        match self.queue.try_send(Queued {
            conversation,
            job,
            cancelled: Arc::clone(&cancelled),
        }) {
            Ok(()) => {
                self.cancels.insert(conversation, cancelled);
                true
            }
            Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) => false,
        }
    }

    fn cancel(&mut self, conversation: ConversationId) {
        if let Some(cancelled) = self.cancels.remove(&conversation) {
            cancelled.store(true, Ordering::Release);
        }
    }
}

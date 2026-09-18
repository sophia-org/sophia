use super::*;
use crate::session_actions::NativeCatalogLaunch;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};

enum CatalogJob {
    Refresh(u64),
    Verify(u64, ApplicationCatalogEntry),
    VerifyNative(Arc<NativeCatalogLaunch>),
}
pub enum ApplicationCatalogWorkerResult {
    Built(u64, Result<ApplicationCatalog, String>),
    Verified(u64, Result<ApplicationLaunchCommand, String>),
    NativeVerified(
        Arc<NativeCatalogLaunch>,
        Result<ApplicationLaunchCommand, String>,
    ),
    Unavailable,
}
pub struct ApplicationCatalogWorker {
    sender: Option<SyncSender<CatalogJob>>,
    thread: Option<std::thread::JoinHandle<()>>,
    joined: Option<bool>,
    receiver: Receiver<ApplicationCatalogWorkerResult>,
    busy: bool,
    disconnected: bool,
}
impl ApplicationCatalogWorker {
    pub fn start(
        config: sophia_config::ApplicationCatalogConfig,
        registered: Vec<RegisteredCatalogApplication>,
        environment: ApplicationCatalogEnvironment,
    ) -> std::io::Result<Self> {
        let (sender, jobs) = sync_channel(1);
        let (results, receiver) = sync_channel(1);
        let thread = std::thread::Builder::new()
            .name("sophia-catalog".into())
            .spawn(move || {
                while let Ok(job) = jobs.recv() {
                    let snapshot = build_application_catalog(&config, &registered, &environment);
                    let result = match job {
                        CatalogJob::Refresh(id) => {
                            ApplicationCatalogWorkerResult::Built(id, snapshot)
                        }
                        CatalogJob::Verify(id, expected) => {
                            let command = verify_snapshot(snapshot, &expected);
                            ApplicationCatalogWorkerResult::Verified(id, command)
                        }
                        CatalogJob::VerifyNative(expected) => {
                            let command = verify_snapshot(snapshot, &expected.entry);
                            ApplicationCatalogWorkerResult::NativeVerified(expected, command)
                        }
                    };
                    if results.send(result).is_err() {
                        break;
                    }
                }
            })?;
        Ok(Self {
            sender: Some(sender),
            thread: Some(thread),
            joined: None,
            receiver,
            busy: false,
            disconnected: false,
        })
    }
    pub fn refresh(&mut self, id: u64) -> bool {
        self.submit(CatalogJob::Refresh(id))
    }
    pub fn verify(&mut self, id: u64, entry: ApplicationCatalogEntry) -> bool {
        self.submit(CatalogJob::Verify(id, entry))
    }
    /// Retains the exact queue payload through filesystem verification. A
    /// successful result is data, not execution authority: the Session must
    /// recheck the current grant and exact queue admission before spawning.
    pub fn verify_native(&mut self, launch: Arc<NativeCatalogLaunch>) -> bool {
        self.submit(CatalogJob::VerifyNative(launch))
    }
    fn submit(&mut self, job: CatalogJob) -> bool {
        if self.busy
            || self
                .sender
                .as_ref()
                .is_none_or(|sender| sender.try_send(job).is_err())
        {
            return false;
        }
        self.busy = true;
        true
    }
    /// Stop new work without blocking the Session owner loop. Any outstanding
    /// result still belongs to this worker and must be consumed through poll.
    pub fn request_shutdown(&mut self) {
        self.sender = None;
    }

    /// Join only a finished thread, after the caller has taken any outstanding
    /// result. Dropping this owner without joining remains abandonment, not a
    /// successful shutdown; callers must retain it while this returns false.
    pub fn poll_shutdown(&mut self) -> Result<bool, &'static str> {
        if let Some(ok) = self.joined {
            return if ok {
                Ok(true)
            } else {
                Err("catalog worker panicked")
            };
        }
        if self.sender.is_some()
            || self.busy
            || self
                .thread
                .as_ref()
                .is_some_and(|thread| !thread.is_finished())
        {
            return Ok(false);
        }
        let ok = self
            .thread
            .take()
            .expect("unjoined catalog worker")
            .join()
            .is_ok();
        self.joined = Some(ok);
        if ok {
            Ok(true)
        } else {
            Err("catalog worker panicked")
        }
    }

    pub fn poll(&mut self) -> Option<ApplicationCatalogWorkerResult> {
        let result = match self.receiver.try_recv() {
            Ok(result) => result,
            Err(std::sync::mpsc::TryRecvError::Empty) => return None,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                if self.disconnected {
                    return None;
                }
                self.disconnected = true;
                ApplicationCatalogWorkerResult::Unavailable
            }
        };
        self.busy = false;
        Some(result)
    }
}

fn verify_snapshot(
    snapshot: Result<ApplicationCatalog, String>,
    expected: &ApplicationCatalogEntry,
) -> Result<ApplicationLaunchCommand, String> {
    let catalog = snapshot?;
    let current = catalog.entries.iter().find(|entry| {
        entry.source == expected.source
            && entry.command == expected.command
            && entry.descriptor.label == expected.descriptor.label
            && entry.descriptor.available
    });
    revalidate_catalog_entry(current.ok_or("catalog changed; reopen launcher")?)
}

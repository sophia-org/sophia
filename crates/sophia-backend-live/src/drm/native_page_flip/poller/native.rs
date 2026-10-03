use super::LibdrmPageFlipEventPoller;
use crate::prelude::*;
use std::{collections::VecDeque, sync::mpsc::SyncSender};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeLibdrmPageFlipEventPoller {
    source: LibdrmNativePageFlipSource,
    routes: Vec<LibdrmNativeOutputRoute>,
    pending_callbacks: VecDeque<LibdrmNativePageFlipCallback>,
    emitted_kernel_timestamps: VecDeque<LibdrmKernelPageFlipTimestamp>,
    read_scratch: Vec<LibdrmNativePageFlipCallback>,
    last_read_loop: LibdrmNativeReadLoopReport,
    cumulative: LibdrmNativePollerCumulativeDiagnostics,
}

impl NativeLibdrmPageFlipEventPoller {
    pub fn new(source: LibdrmNativePageFlipSource) -> Self {
        Self {
            source,
            routes: Vec::new(),
            pending_callbacks: VecDeque::new(),
            emitted_kernel_timestamps: VecDeque::new(),
            read_scratch: Vec::new(),
            last_read_loop: LibdrmNativeReadLoopReport::idle(),
            cumulative: LibdrmNativePollerCumulativeDiagnostics::default(),
        }
    }

    pub fn with_routes(
        mut self,
        routes: impl IntoIterator<Item = LibdrmNativeOutputRoute>,
    ) -> Self {
        self.replace_routes(routes);
        self
    }

    pub fn replace_routes(&mut self, routes: impl IntoIterator<Item = LibdrmNativeOutputRoute>) {
        self.routes.clear();
        self.routes.extend(routes);
        self.pending_callbacks.reserve(self.routes.len());
        self.emitted_kernel_timestamps.reserve(self.routes.len());
        self.read_scratch.reserve(self.routes.len());
    }

    pub fn inject_callbacks(
        &mut self,
        callbacks: impl IntoIterator<Item = LibdrmNativePageFlipCallback>,
    ) {
        self.pending_callbacks.extend(callbacks);
    }

    pub fn read_page_flip_events<R>(
        &mut self,
        reader: &mut R,
        max_read: usize,
    ) -> LibdrmNativeReadLoopReport
    where
        R: LibdrmNativePageFlipReader,
    {
        self.read_scratch.clear();
        let report = reader.read_ready_page_flip_callbacks_into(max_read, &mut self.read_scratch);
        self.last_read_loop = report;
        self.cumulative.read_calls = self.cumulative.read_calls.saturating_add(1);
        self.cumulative.decoded_callbacks = self
            .cumulative
            .decoded_callbacks
            .saturating_add(report.decoded_callbacks);
        self.cumulative.rejected_callbacks = self
            .cumulative
            .rejected_callbacks
            .saturating_add(report.rejected_callbacks);
        match report.status {
            LibdrmNativeReadLoopStatus::WouldBlock => {
                self.cumulative.would_block_reads =
                    self.cumulative.would_block_reads.saturating_add(1);
            }
            LibdrmNativeReadLoopStatus::ReadFailed => {
                self.cumulative.read_failures = self.cumulative.read_failures.saturating_add(1);
            }
            _ => {}
        }
        if report.status != LibdrmNativeReadLoopStatus::ReadFailed {
            self.pending_callbacks.extend(self.read_scratch.drain(..));
        }
        report
    }

    pub fn read_and_poll_page_flip_events<R>(
        &mut self,
        reader: &mut R,
        sender: &SyncSender<LivePageFlipCallback>,
        max_read: usize,
        max_emit: usize,
    ) -> LibdrmNativeReadAndPollReport
    where
        R: LibdrmNativePageFlipReader,
    {
        if !self.pending_callbacks.is_empty() {
            let poll = self.poll_page_flip_events(sender, max_emit);
            return LibdrmNativeReadAndPollReport {
                read_loop: self.last_read_loop,
                poll,
            };
        }

        let read_loop = self.read_page_flip_events(reader, max_read);
        if read_loop.status == LibdrmNativeReadLoopStatus::ReadFailed {
            return LibdrmNativeReadAndPollReport {
                read_loop,
                poll: read_loop.into_poll_report(),
            };
        }

        if self.pending_callbacks.is_empty() {
            return LibdrmNativeReadAndPollReport {
                read_loop,
                poll: read_loop.into_poll_report(),
            };
        }

        LibdrmNativeReadAndPollReport {
            read_loop,
            poll: self.poll_page_flip_events(sender, max_emit),
        }
    }

    pub const fn source_report(&self) -> LibdrmNativePageFlipSourceReport {
        self.source.report()
    }

    pub const fn last_read_loop_report(&self) -> LibdrmNativeReadLoopReport {
        self.last_read_loop
    }

    pub fn pending_callback_count(&self) -> usize {
        self.pending_callbacks.len()
    }

    pub fn drain_emitted_kernel_timestamps(&mut self) -> Vec<LibdrmKernelPageFlipTimestamp> {
        self.emitted_kernel_timestamps.drain(..).collect()
    }

    pub fn route_count(&self) -> usize {
        self.routes.len()
    }

    pub const fn cumulative_diagnostics(&self) -> LibdrmNativePollerCumulativeDiagnostics {
        self.cumulative
    }

    pub fn read_and_collect_page_flip_events<R>(
        &mut self,
        reader: &mut R,
        callbacks: &mut Vec<LivePageFlipCallback>,
        timestamps: &mut Vec<LibdrmKernelPageFlipTimestamp>,
        max_read: usize,
        max_emit: usize,
    ) -> LibdrmNativeReadAndPollReport
    where
        R: LibdrmNativePageFlipReader,
    {
        let read_loop = if self.pending_callbacks.is_empty() {
            self.read_page_flip_events(reader, max_read)
        } else {
            self.last_read_loop
        };
        if read_loop.status == LibdrmNativeReadLoopStatus::ReadFailed {
            return LibdrmNativeReadAndPollReport {
                read_loop,
                poll: read_loop.into_poll_report(),
            };
        }

        let mut emitted = 0usize;
        let mut rejected = 0usize;
        for _ in 0..max_emit {
            let Some(native) = self.pending_callbacks.pop_front() else {
                break;
            };
            let decoded = native.decode(&self.routes);
            let Some(callback) = decoded.callback else {
                rejected = rejected.saturating_add(1);
                continue;
            };
            if let Some(ust_usec) = native.kernel_ust_usec() {
                timestamps.push(LibdrmKernelPageFlipTimestamp {
                    output: callback.output,
                    head: callback.head,
                    frame_serial: callback.frame_serial,
                    kernel_sequence: native.kernel_sequence(),
                    ust_usec,
                });
            }
            callbacks.push(callback);
            emitted = emitted.saturating_add(1);
        }
        self.cumulative.emitted_callbacks =
            self.cumulative.emitted_callbacks.saturating_add(emitted);
        self.cumulative.rejected_callbacks =
            self.cumulative.rejected_callbacks.saturating_add(rejected);
        let queued_remaining = self.pending_callbacks.len();
        let source = LivePageFlipCallbackSourceReport {
            emitted,
            queued_remaining,
            backpressure: false,
            disconnected: false,
            max_reached: queued_remaining > 0,
        };
        LibdrmNativeReadAndPollReport {
            read_loop,
            poll: LibdrmPageFlipEventPollReport::from_source_report(source),
        }
    }

    pub fn diagnostics(&self) -> LibdrmNativePollerDiagnostics {
        LibdrmNativePollerDiagnostics {
            route_count: self.routes.len(),
            pending_callbacks: self.pending_callbacks.len(),
            last_read_loop: self.last_read_loop,
        }
    }
}

impl LibdrmPageFlipEventPoller for NativeLibdrmPageFlipEventPoller {
    fn poll_page_flip_events(
        &mut self,
        sender: &SyncSender<LivePageFlipCallback>,
        max_emit: usize,
    ) -> LibdrmPageFlipEventPollReport {
        let _ = self.source.report();
        if self.pending_callbacks.is_empty() {
            self.last_read_loop = LibdrmNativeReadLoopReport::idle();
            return self.last_read_loop.into_poll_report();
        }

        let pending = self.pending_callbacks.iter().copied().collect::<Vec<_>>();
        let report = decode_native_page_flip_batch(&pending, &self.routes, sender, max_emit);
        let processed_callbacks = pending
            .len()
            .saturating_sub(report.poll.callbacks.queued_remaining);

        for native in pending.iter().take(processed_callbacks).copied() {
            let Some(ust_usec) = native.kernel_ust_usec() else {
                continue;
            };
            let Some(callback) = native.decode(&self.routes).callback else {
                continue;
            };
            self.emitted_kernel_timestamps
                .push_back(LibdrmKernelPageFlipTimestamp {
                    output: callback.output,
                    head: callback.head,
                    frame_serial: callback.frame_serial,
                    kernel_sequence: native.kernel_sequence(),
                    ust_usec,
                });
        }
        for _ in 0..processed_callbacks {
            let _ = self.pending_callbacks.pop_front();
        }

        self.cumulative.emitted_callbacks = self
            .cumulative
            .emitted_callbacks
            .saturating_add(report.poll.callbacks.emitted);

        self.last_read_loop = report.read_loop;
        report.poll
    }
}

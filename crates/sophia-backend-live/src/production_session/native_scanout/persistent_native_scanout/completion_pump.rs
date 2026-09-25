impl LiveProductionNativeScanout {
        pub fn poll_group_callbacks(
            &mut self,
            group: usize,
        ) -> Result<(), Box<dyn std::error::Error>> {
            let callback_capacity = self
                .heads
                .iter()
                .filter(|head| head.enabled && head.group == group)
                .count()
                .max(1);
            let report = {
                let owner = &mut self.groups[group];
                owner.callbacks.clear();
                owner.timestamps.clear();
                owner.session.collect_native_page_flip_events(
                    &mut owner.callbacks,
                    &mut owner.timestamps,
                    callback_capacity,
                    callback_capacity,
                )
            };
            if report.read_loop.status == crate::LibdrmNativeReadLoopStatus::ReadFailed {
                return Err("native card page-flip read failed".into());
            }
            let mut callbacks = std::mem::take(&mut self.groups[group].callbacks);
            let mut timestamps = std::mem::take(&mut self.groups[group].timestamps);
            let route_result = (|| -> Result<(), Box<dyn std::error::Error>> {
                for timestamp in &mut timestamps {
                    let head = self
                        .heads
                        .iter()
                        .find(|head| {
                            head.group == group && head.enabled && head.head == timestamp.head
                        })
                        .ok_or("native timestamp referenced an inactive or unknown head")?;
                    // The libdrm route was created at discovery time. Dynamic
                    // regrouping changes only the logical output, so normalize
                    // that policy identity through the current opaque head.
                    timestamp.output = head.output.id;
                    self.kernel_page_flip_ust.insert(
                        (timestamp.output, timestamp.head, timestamp.frame_serial),
                        timestamp.ust_usec,
                    );
                }
                for callback in &mut callbacks {
                    // By head, not by output. Two heads of a mirror group share
                    // an output, so only the head identifies the physical owner.
                    let Some(head_index) = self.heads.iter().position(|head| {
                        head.group == group && head.enabled && head.head == callback.head
                    }) else {
                        return Err(format!(
                            "native callback referenced an unknown head: head={} output={}",
                            callback.head.raw(),
                            callback.output.raw(),
                        )
                        .into());
                    };
                    callback.output = self.heads[head_index].output.id;
                    if self.heads[head_index].completion_mode
                        == LiveProductionKmsCompletionMode::OutFenceAuthoritative
                    {
                        self.heads[head_index].late_page_flip_events = self.heads[head_index]
                            .late_page_flip_events
                            .saturating_add(1);
                        self.kernel_page_flip_ust.remove(&(
                            callback.output,
                            callback.head,
                            callback.frame_serial,
                        ));
                        continue;
                    }
                    if self.heads[head_index].pending_callback.is_some() {
                        self.callback_queue_saturated =
                            self.callback_queue_saturated.saturating_add(1);
                        return Err(format!(
                            "native head completion ledger is full: head={} output={}",
                            callback.head.raw(),
                            callback.output.raw(),
                        )
                        .into());
                    }
                    self.heads[head_index].pending_callback = Some(*callback);
                }
                Ok(())
            })();
            callbacks.clear();
            timestamps.clear();
            self.groups[group].callbacks = callbacks;
            self.groups[group].timestamps = timestamps;
            route_result
        }

        /// Poll every DRM card before any output retirement or watchdog check.
        pub fn pump_native_completions(&mut self) -> Result<(), Box<dyn std::error::Error>> {
            for group in 0..self.groups.len() {
                self.poll_group_callbacks(group)?;
            }
            Ok(())
        }
}

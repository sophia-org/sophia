type OutputKmsReadback = Vec<sophia_backend_live::LiveProductionOutputKmsReadback>;

/// Device reads are opt-in and use the existing native owner. The captured
/// before-state is retained through commit or observed rollback, never inferred
/// from the topology object (which deliberately stays unchanged during apply).
struct OutputReadbackProof {
    enabled: bool,
    baseline_recorded: bool,
    peer_exit_recorded: bool,
    before: Option<OutputReadbackCandidate>,
    failure: Option<String>,
}

struct OutputReadbackCandidate {
    connection_epoch: u64,
    base_topology_epoch: u64,
    transaction: TransactionId,
    kms: OutputKmsReadback,
    owner: OutputOwnerReadback,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct OutputOwnerHeadReadback {
    head: sophia_engine::RenderHeadId,
    enabled: bool,
    output: sophia_engine::HeadlessOutput,
    scale: u32,
    refresh_millihz: u32,
    transform: sophia_protocol::OutputTransform,
    mapping: sophia_protocol::OutputHeadMapping,
    vrr: sophia_protocol::OutputVrrPolicy,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct OutputOwnerReadback {
    heads: Vec<OutputOwnerHeadReadback>,
    outputs: Vec<sophia_engine::HeadlessOutput>,
}

fn read_output_owner(native: &LiveProductionNativeScanout) -> OutputOwnerReadback {
    let mut heads = native
        .heads
        .iter()
        .map(|head| OutputOwnerHeadReadback {
            head: head.head,
            enabled: head.enabled,
            output: head.output,
            scale: head.scale,
            refresh_millihz: head.refresh_millihz,
            transform: head.transform,
            mapping: head.mapping,
            vrr: head.vrr,
        })
        .collect::<Vec<_>>();
    heads.sort_by_key(|head| head.head);
    let mut outputs = native.outputs();
    outputs.sort_by_key(|output| output.id);
    OutputOwnerReadback { heads, outputs }
}

impl OutputReadbackProof {
    const fn new(enabled: bool) -> Self {
        Self {
            enabled,
            baseline_recorded: false,
            peer_exit_recorded: false,
            before: None,
            failure: None,
        }
    }

    fn baseline(
        &mut self,
        native: &LiveProductionNativeScanout,
        epoch: u64,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if self.enabled && !self.baseline_recorded {
            trace_output_kms_readback(
                "baseline",
                0,
                epoch,
                None,
                &native.output_topology_kms_readback()?,
                &read_output_owner(native),
            );
            self.baseline_recorded = true;
        }
        Ok(())
    }

    fn peer_exited(
        &mut self,
        native: &LiveProductionNativeScanout,
        epoch: u64,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if self.enabled
            && self.baseline_recorded
            && !self.peer_exit_recorded
            && self.before.is_none()
        {
            trace_output_kms_readback(
                "peer_exit",
                0,
                epoch,
                None,
                &native.output_topology_kms_readback()?,
                &read_output_owner(native),
            );
            self.peer_exit_recorded = true;
        }
        Ok(())
    }

    fn begin(
        &mut self,
        native: &LiveProductionNativeScanout,
        epoch: u64,
        base_topology_epoch: u64,
        transaction: TransactionId,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if !self.enabled {
            return Ok(());
        }
        if self.before.is_some() {
            return Err("output readback proof already owns a candidate".into());
        }
        let before = native.output_topology_kms_readback()?;
        if before.is_empty() {
            return Err("output proof has no selected KMS heads".into());
        }
        let owner = read_output_owner(native);
        trace_output_kms_readback(
            "before",
            epoch,
            base_topology_epoch,
            Some(transaction),
            &before,
            &owner,
        );
        self.before = Some(OutputReadbackCandidate {
            connection_epoch: epoch,
            base_topology_epoch,
            transaction,
            kms: before,
            owner,
        });
        Ok(())
    }

    fn observe(
        &self,
        native: &LiveProductionNativeScanout,
        stage: &str,
        transaction: TransactionId,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(before) = &self.before
            && before.transaction == transaction
        {
            let actual = native.output_topology_kms_readback()?;
            let owner = read_output_owner(native);
            trace_output_kms_readback(
                stage,
                before.connection_epoch,
                before.base_topology_epoch,
                Some(transaction),
                &actual,
                &owner,
            );
            before.check(stage, &actual, &owner)?;
        }
        Ok(())
    }

    fn settled(&mut self, transaction: TransactionId) {
        if self
            .before
            .as_ref()
            .is_some_and(|before| before.transaction == transaction)
        {
            self.before = None;
        }
    }

    fn fail(&mut self, transaction: TransactionId, error: impl std::fmt::Display) {
        if self.failure.is_none() {
            tracing::error!(
                "sophia_output_readback_proof schema=1 status=failed transaction={} reason={error}",
                transaction.raw()
            );
            self.failure = Some(error.to_string());
        }
    }

    fn rollback_required(&self, transaction: TransactionId) -> bool {
        self.failure.is_some()
            && self
                .before
                .as_ref()
                .is_some_and(|before| before.transaction == transaction)
    }

    fn check_finished(&self) -> Result<(), Box<dyn std::error::Error>> {
        if self.before.is_none()
            && let Some(error) = &self.failure
        {
            return Err(format!("output readback proof failed after settlement: {error}").into());
        }
        Ok(())
    }
}

impl OutputReadbackCandidate {
    fn check(
        &self,
        stage: &str,
        actual: &[sophia_backend_live::LiveProductionOutputKmsReadback],
        owner: &OutputOwnerReadback,
    ) -> Result<(), &'static str> {
        if stage == "restored" && actual != self.kms {
            return Err("output rollback KMS readback differs from its pre-apply state");
        }
        if stage == "restored" && *owner != self.owner {
            return Err("output rollback native owner state differs from its pre-apply state");
        }
        if stage == "installed"
            && (actual
                .iter()
                .map(|head| (head.head, head.card, head.connector, head.crtc, head.plane))
                .ne(self
                    .kms
                    .iter()
                    .map(|head| (head.head, head.card, head.connector, head.crtc, head.plane)))
                || owner
                    .heads
                    .iter()
                    .map(|head| (head.head, head.enabled))
                    .ne(self
                        .owner
                        .heads
                        .iter()
                        .map(|head| (head.head, head.enabled))))
        {
            return Err(
                "output native proof requires unchanged head enablement and KMS selections",
            );
        }
        // Software transform-only trials need separate pixel-level acceptance.
        if stage == "applied"
            && actual
                .iter()
                .map(|head| (head.head, head.mode))
                .eq(self.kms.iter().map(|head| (head.head, head.mode)))
        {
            return Err("output native proof did not change KMS mode timing");
        }
        Ok(())
    }
}

fn trace_output_kms_readback(
    stage: &str,
    connection_epoch: u64,
    base_topology_epoch: u64,
    transaction: Option<TransactionId>,
    records: &[sophia_backend_live::LiveProductionOutputKmsReadback],
    owner: &OutputOwnerReadback,
) {
    let clock = rustix::time::clock_gettime(rustix::time::ClockId::Monotonic);
    let ns = i128::from(clock.tv_sec) * 1_000_000_000 + i128::from(clock.tv_nsec);
    for record in records {
        let mode = record.mode.map_or_else(
            || "disabled".to_owned(),
            |mode| {
                mode.iter()
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join(",")
            },
        );
        let properties = record
            .properties
            .iter()
            .map(|(name, value)| format!("{name}:{value}"))
            .collect::<Vec<_>>()
            .join(",");
        tracing::info!(
            "sophia_output_kms_readback schema=1 stage={stage} t={ns} connection_epoch={connection_epoch} base_topology_epoch={base_topology_epoch} transaction={} head={} card={} connector={} crtc={} plane={} mode={mode} properties={properties}",
            transaction.map_or(0, TransactionId::raw),
            record.head.raw(),
            record.card,
            record.connector,
            record.crtc,
            record.plane,
        );
    }
    tracing::info!(
        "sophia_output_kms_readback schema=1 stage={stage} t={ns} connection_epoch={connection_epoch} base_topology_epoch={base_topology_epoch} transaction={} heads={} complete=true",
        transaction.map_or(0, TransactionId::raw),
        records.len()
    );
    for head in &owner.heads {
        tracing::info!(
            "sophia_output_owner_readback schema=1 stage={stage} t={ns} connection_epoch={connection_epoch} base_topology_epoch={base_topology_epoch} transaction={} head={} enabled={} output={} native_width={} native_height={} native_scale={} scale={} refresh_millihz={} transform={:?} mapping={:?} vrr={:?}",
            transaction.map_or(0, TransactionId::raw),
            head.head.raw(),
            head.enabled,
            head.output.id.raw(),
            head.output.size.width,
            head.output.size.height,
            head.output.scale,
            head.scale,
            head.refresh_millihz,
            head.transform,
            head.mapping,
            head.vrr
        );
    }
    for output in &owner.outputs {
        tracing::info!(
            "sophia_output_owner_readback schema=1 stage={stage} t={ns} connection_epoch={connection_epoch} base_topology_epoch={base_topology_epoch} transaction={} output={} logical_width={} logical_height={} scale={}",
            transaction.map_or(0, TransactionId::raw),
            output.id.raw(),
            output.size.width,
            output.size.height,
            output.scale
        );
    }
    tracing::info!(
        "sophia_output_owner_readback schema=1 stage={stage} t={ns} connection_epoch={connection_epoch} base_topology_epoch={base_topology_epoch} transaction={} heads={} outputs={} complete=true",
        transaction.map_or(0, TransactionId::raw),
        owner.heads.len(),
        owner.outputs.len()
    );
}

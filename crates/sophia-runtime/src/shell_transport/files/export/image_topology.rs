//! Prepare fallible snapshots before the role commits topology; install them
//! before its existing notification. Open fids retain the previous objects.
use super::*;

pub(in crate::shell_transport) struct DiscoverySnapshot {
    pub qid: u64,
    pub text: String,
}

pub(in crate::shell_transport) struct ImageTopology {
    discovery: ShellImageDiscovery,
    formats: ShellImageFormats,
    discovery_snapshot: Arc<DiscoverySnapshot>,
    formats_body: Vec<u8>,
    formats_snapshot: Arc<Object>,
}

impl ShellFiles {
    pub(in crate::shell_transport) fn prepare_image_topology(
        &mut self,
        generation: u64,
    ) -> Result<Option<ImageTopology>, Errno> {
        // Facts can advance before negotiation. Keep the hidden configuration
        // current too, so its first disclosed snapshots name those facts.
        let Some(state) = self.image.as_ref() else {
            return Ok(None);
        };
        if generation < state.config.formats.formats_generation {
            return Err(Errno::ESTALE);
        }
        if generation == state.config.formats.formats_generation {
            return Ok(None);
        }
        let mut discovery = state.config.discovery.clone();
        let mut formats = state.config.formats.clone();
        discovery.formats_generation = generation;
        formats.formats_generation = generation;
        let text = discovery.encode().map_err(|_| Errno::EINVAL)?;
        let formats_body = formats.encode().map_err(|_| Errno::EINVAL)?;
        let bytes = encode_shell_file_record_at_revision(
            ShellFileHeader {
                kind: ShellFileKind::ImageFormats,
                connection_epoch: self.epoch,
                submission_id: 0,
                sequence: 0,
            },
            &formats_body,
            9,
        )
        .map_err(|_| Errno::EINVAL)?;
        let discovery_snapshot = Arc::new(DiscoverySnapshot {
            qid: self.allocate_qid()?,
            text,
        });
        let formats_snapshot = Arc::new(Object {
            kind: ShellFileKind::ImageFormats,
            qid: self.allocate_qid()?,
            bytes,
        });
        Ok(Some(ImageTopology {
            discovery,
            formats,
            discovery_snapshot,
            formats_body,
            formats_snapshot,
        }))
    }

    pub(in crate::shell_transport) fn install_image_topology(&mut self, prepared: ImageTopology) {
        let state = self.image.as_mut().expect("prepared image topology");
        state.config.discovery = prepared.discovery;
        state.config.formats = prepared.formats;
        state.discovery = prepared.discovery_snapshot;
        state.formats_body = prepared.formats_body;
        self.object_slot_mut(ShellFileKind::ImageFormats)
            .expect("image formats slot")
            .current = Some(prepared.formats_snapshot);
    }
}

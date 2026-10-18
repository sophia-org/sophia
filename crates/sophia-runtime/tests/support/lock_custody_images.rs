//! Supplied worker completion for custody tests. Real descriptor and sealing
//! checks live in lock_image_transport; this fixture preserves presentation tests.
use super::*;
use sophia_runtime::SharedPixels;
use std::sync::Arc;

fn image_settings() -> LockImageSettings {
    LockImageSettings {
        discovery: LockImageDiscovery {
            connection_epoch: EPOCH,
            channel_generation: 5,
            endpoint: "image.sock".into(),
            source_kinds: 1,
            max_offers: 2,
            max_packets_per_turn: 8,
            hello_timeout_ms: 1000,
            validation_timeout_ms: 100,
            validation_chunk_bytes: 65536,
            capture_interest_timeout_ms: 200,
            formats_generation: 3,
            limits_generation: 3,
        },
        limits: LockImageLimits {
            channel_generation: 5,
            limits_generation: 3,
            max_resource_bytes: 64 * 64 * 4,
            max_sealed_backing_bytes: 2 * 64 * 64 * 4,
            max_dma_source_bytes: 0,
            max_renderer_allocation_bytes: 0,
            max_renderer_entries: 0,
            max_offers: 2,
            max_validation_jobs: 2,
            max_capture_jobs: 0,
            max_admitted_outputs: 1,
            max_format_rows_per_output: 1,
            max_width: 64,
            max_height: 64,
            validation_timeout_ms: 100,
            capture_interest_timeout_ms: 200,
            validation_chunk_bytes: 65536,
            max_packets_per_turn: 8,
        },
        formats: LockImageFormats {
            channel_generation: 5,
            formats_generation: 3,
            limits_generation: 3,
            rows: Vec::new(),
        },
    }
}

impl Provider {
    pub(super) fn image_ready(&mut self) {
        let topology = self.custody.lock().0.topology_generation;
        let mut settings = image_settings();
        settings.formats.formats_generation = topology;
        settings.discovery.formats_generation = topology;
        self.custody.configure_image(settings).unwrap();
        let body = LockNegotiate {
            minimum_revision: 2,
            maximum_revision: 2,
            requested_capabilities: LOCK_FILE_CAPABILITY_PRESENT
                | LOCK_FILE_CAPABILITY_CHORDS
                | (1 << 2),
            chords: Vec::new(),
        }
        .encode()
        .unwrap();
        self.submit(LockFileKind::Negotiate, &body).unwrap();
        self.custody.image_channel_ready(5, 1).unwrap();
    }

    pub(super) fn upload(&mut self, id: u64) {
        let lock = self.custody.lock().0;
        let width = lock.allocations[0].pixel_width;
        let height = lock.allocations[0].pixel_height;
        let bytes = u64::from(width) * u64::from(height) * 4;
        let m = LockImageMetadata {
            offer_id: id,
            resource: id,
            resource_generation: 1,
            admission_epoch: lock.lock_epoch,
            output_id: 1,
            allocation_id: 11,
            topology_generation: lock.topology_generation,
            source_kind: 1,
            source_format: 1,
            width,
            height,
            stride: width * 4,
            plane_count: 1,
            descriptor_count: 1,
            declared_bytes: bytes,
            modifier: 0,
        };
        self.submit(LockFileKind::ImageBegin, &m.encode().unwrap())
            .unwrap();
        let admission = self.custody.image_admission(self.now).unwrap().unwrap();
        self.custody
            .admit_image(id, id, admission.deadline)
            .unwrap();
        let mut planes = [LockImagePlane {
            offset: 0,
            stride: 0,
            fd_index: 0,
        }; LOCK_IMAGE_MAX_PLANES];
        planes[0].stride = width * 4;
        let source = LockImagePacket::Source {
            connection_epoch: EPOCH,
            channel_generation: 5,
            metadata: m,
            planes,
        };
        assert_eq!(
            self.custody.image_source(&source, 1),
            Ok(LockImageSource::Submit { job: id })
        );
        self.custody.image_source_submitted(id).unwrap();
        self.custody
            .image_completed(
                id,
                Ok(SharedPixels::from(Arc::<[u8]>::from(vec![
                    7;
                    bytes as usize
                ]))),
            )
            .unwrap();
        self.events();
        assert!(matches!(
            self.custody.take_inbound(),
            Some(LockInbound::ResourceReady { .. })
        ));
    }
}

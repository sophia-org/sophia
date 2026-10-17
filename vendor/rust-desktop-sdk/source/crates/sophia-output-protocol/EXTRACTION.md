# Output protocol extraction

Source: signed Sophia `ad2fa4962ba1e7626b601e1a5b6b0ab95324bfc3`. File codecs and passive output values
are copied without authority owners. Tests change only the crate name and schema
path. The codec error omits unrelated shell conversions; the cursor omits unused
single-byte helpers. Geometry and opaque head/mode IDs are selected from their
source modules; output/transaction IDs use the shared desktop IDs. The output
module comment is corrected to distinguish codec and session responsibilities.
The output schema and contract are byte-identical to the C SDK pins.

Original source hashes (before the adaptations above):

- `crates/sophia-protocol/src/output_files.rs`: `cc4415d9d9ff1a3dc52a2be0c2e993588444ea58e18021e218ec496b3e03543e`
- `crates/sophia-protocol/src/output_role.rs`: `9d17813d09ff2bed74c4911c3d2051fb99f52618982499471f833d6e3de5aef5`
- `crates/sophia-protocol/src/byte_cursor.rs`: `f3c8df09e2316ec6891529480111b96fa47f38a461324b93c1958633ff3c0b53`
- `crates/sophia-protocol/src/codec_error.rs`: `7696a32183331bff7fda29e3038f5e2e7ed016c151dcdb8203f5f62b43990adf`
- `crates/sophia-protocol/src/output_files/controls.rs`: `8e3f6ce175d01f60f146fcead4dc3b970ef865412c9ad6aa0476ad217d87e6fe`
- `crates/sophia-protocol/src/output_files/envelope.rs`: `571a0a046eb1a725bbfb22a0b65f018a1d09d9b8f1ac81d6562b5b979ed1a8b3`
- `crates/sophia-protocol/src/output_files/events.rs`: `bfb9a2da0d1ab104d73bddde4ba5a6196b7aa38797930b083cc4c37a22e20c60`
- `crates/sophia-protocol/src/output_files/limits.rs`: `e2e8ce2e1d4ca754849e92fcb1e490a45bb2875b23e83af8a805a35c565d1864`
- `crates/sophia-protocol/src/output_files/proposal.rs`: `ba8c0e81de522efd06cf1416e7d8e72002fe574af78ea381f4814a8be10c36d5`
- `crates/sophia-protocol/src/output_files/topology.rs`: `2748d3b06f91972bd3ebd2f06b16250a054bd09c39ad142ffa4cf7072c21425b`
- `crates/sophia-protocol/src/packets/output_configuration.rs`: `18276ceedfa3070c45ae0263f0698a0ef8bfa8bfcf76a031a3f1cdcec2ea5aca`
- `crates/sophia-protocol/tests/output_files.rs`: `cef479c94d9f321e6cd7338084a6fd5911fdd80a369197f729d5c31e67ad0ac4`
- `crates/sophia-protocol/tests/output_file_events.rs`: `79f57f26aa044f691737d67828165c33756cda0ad242386258bab9d3fd9cff1d`
- `crates/sophia-protocol/tests/output_file_topology.rs`: `01748429bc357d0884ce86a1b9c90df9105c834960aff2a7400c9c75cfe8e845`
- `crates/sophia-protocol/tests/output_file_limits.rs`: `9d5809e289b2d5eb361c92b92cbddea73511a0bc7c0ea403e8efb85c91fec867`
- `crates/sophia-protocol/tests/output_file_schema.rs`: `c6dff2800315079fbb0c2dab772ab706d6ad77964151b3f8af139c90ec7e7e56`
- `crates/sophia-protocol/tests/support/file_schema.rs`: `ce1e8d9403cd5352001e0a5ccfe902b3509a77c2bf9dc4873ca8044939214e2b`
- `crates/sophia-protocol/src/ids.rs`: `2d49429dbf155d30b6d4ac12a5d41c0618e57d554cc688f5592ae67ea14c93f1`
- `crates/sophia-protocol/src/geometry.rs`: `27d0e7443da4baf8421de38ac22c54cecee92ca23e41cc0e477abbbe6fa77d99`
- `protocol/sophia-output-files-v1.kdl`: `c4b82e748017a1f660e4e7a03f828fe7b3cbe44a9d2aeee1fbcd13dd7877ab0d`
- `docs/sophia-output-files.md`: `50b6342fcf6398c32025d8329e87d0818d661c5c11be77b36a70fe93cdeea3cf`

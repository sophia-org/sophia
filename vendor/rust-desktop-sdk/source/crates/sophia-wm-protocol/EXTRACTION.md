# WM protocol extraction

Source: Sophia `1d071c144199548b55d8c42a961aa3e13b60ef74`. WM codec modules and passive policy models are copied;
IDs, geometry and support values are selected from their owning modules.
The codec error owner omits conversions for unrelated shell/Engine validation.
The cursor omits unused single-byte helpers; all WM codecs remain unchanged.
No Engine, X11 or IPC module is imported.

Source hashes for whole-file imports:

- `crates/sophia-protocol/src/wm_files.rs`: `04760f6a666026bc510bed154d91c3d78c9bc14e0564b665f4344841d856b531`
- `crates/sophia-protocol/src/wm_records.rs`: `a07234f64bcb9320533671e1f4999516e6c316ee75851b325e818dd11ffd0897`
- `crates/sophia-protocol/src/wm_rows.rs`: `1fd1ee5f10c940a5abbc37319ba6a9a22692c4cb64fcc909604d005b581fa436`
- `crates/sophia-protocol/src/policy_scalars.rs`: `46d29ce6dd3857101a8d3a291989c8bc0389d4434a08cfe437e985f2d5a6ac31`
- `crates/sophia-protocol/src/policy_profile.rs`: `9a30bfc0935496b422ba4eced6ca688b00b67deca1ed4665068a449a452dbe3c`
- `crates/sophia-protocol/src/byte_cursor.rs`: `f3c8df09e2316ec6891529480111b96fa47f38a461324b93c1958633ff3c0b53`
- `crates/sophia-protocol/src/wm_files/admission.rs`: `50104cc1a24d9616ae2c28a980bdf9cae777a6801587c4c8663c2aba95f5b2e8`
- `crates/sophia-protocol/src/wm_files/arrays.rs`: `b129526fcf28fcbac5db2f90016cdccbd497f08bd161aba2acc32c8ccd9c00d1`
- `crates/sophia-protocol/src/wm_files/codec.rs`: `499d7010bc7eb05e26710a8d55f5547b703d389550d49c46a15c859170bbf813`
- `crates/sophia-protocol/src/wm_files/controls.rs`: `daa7712f19084e8c786b40f358ceec132e8f5e5c42514594ffdf2c3038dc3e6b`
- `crates/sophia-protocol/src/wm_files/cycle.rs`: `c4ee31fc9df24b51b6e8dfa0380ff80894158780137bb2affa51bf8d73c1f6dd`
- `crates/sophia-protocol/src/wm_files/payload.rs`: `418bc88683dd3e80d29a0b65045b8b611c3904ccb7b7c82152147d95d4590023`
- `crates/sophia-protocol/src/wm_files/records.rs`: `4c63599ecc1d8360ab967766b3560d429354f8b38ff993cd7595cb3dbcb1067a`
- `crates/sophia-protocol/src/wm_records/configuration.rs`: `fda2c2f6d1a3af921084c84bc43cf21ac6c0154c58709ff01257511830e8797e`
- `crates/sophia-protocol/src/wm_records/launch_origins.rs`: `d6d1de0dee9fa1823828ab4c82ab26ff4c61d1bd4f0aae1313faeca6932a14a6`
- `crates/sophia-protocol/src/wm_records/output_launch_contexts.rs`: `37e61faceb4f284e93ae1fa7b6be8930b0091820f3d882378279eef57e3da83e`
- `crates/sophia-protocol/src/wm_records/output_policy_keys.rs`: `0dd5c53dcac0f08e228c18eb5c41bf2d8c26967b2dc83e166b4329eee9a710f2`
- `crates/sophia-protocol/src/wm_records/presentation.rs`: `b2e9de1e42fbace70676b54e7daf02134fe6084883483a84a6000068edc4f1e9`
- `crates/sophia-protocol/src/wm_records/projection.rs`: `b09047c28d589a6710282ad844d440000efe0992b757b6e258f0384f02d09656`
- `crates/sophia-protocol/src/wm_records/sections.rs`: `90bdc1edb8186345c3daf4f689a1839218a22c80bbeb10873ddd19fdc84491e1`
- `crates/sophia-protocol/src/wm_records/snapshot.rs`: `73579ddd7641dcd25bbfe32f0175899613f97bc46406451f3ad9e9533654bac6`
- `crates/sophia-protocol/src/wm_records/tab_groups.rs`: `501101efac126576c2616a9d8a51c893a44885c2136e4d7ea332f713fcd86031`
- `crates/sophia-protocol/src/wm_records/translation.rs`: `698c78a13e07b07de5f77c0cf4ad6b1cd25e157ed9a891461d8e586ca11eab5d`
- `crates/sophia-protocol/src/wm_records/values.rs`: `844bacc4b1ccee66ecfbd94fe34cfeb1acd661f3f0dc260e56f8526fc1bb3cb6`
- `crates/sophia-protocol/src/packets/policy.rs`: `c379be6df1c61c56471a00a606a9bf7e62cba76439b20e303bf2780b9584b237`
- `crates/sophia-protocol/src/packets/presentation.rs`: `29ee0974c8f12e9e8ca6c5f08850e6e11edc46fb6b1ece42a19bc613de9bcfd6`
- `crates/sophia-protocol/src/packets/tab_groups.rs`: `15d1fd7f7223655a66b6be09e7b5aa7e1653d2cf3c185b4261e56cb91869b93f`
- `protocol/sophia-wm-files-v1.kdl`: `702288697c0a6deba54245aba1cbe9530ef55e3f0dc8cf2622ca52e34b9fcb85`
- `docs/sophia-wm-api.md`: `58c3806f72bb80c6b29648ebe34ad2e24706759fdbab689e21742b0876c80c58`
- `docs/sophia-wm-files.md`: `0476cbf5965061dfc57aef8d26c1ee338182ea6535f4f8b53d2520b79ebc2c9a`
- `protocol/golden/sophia-wm-v1.records`: `709a7cafc0a64bae0b3464167cb809dbc67a6fe600ffa4885346b632d21f68ee`
- `crates/sophia-protocol/src/presentation.rs`: `2c7bc5e1824630781b5b20eab7cdd2d4083762f6d5997f97e0ea3d1fd36f11bf`

Selected support values come from these source files at the same revision:

- `crates/sophia-protocol/src/ids.rs`: `2d49429dbf155d30b6d4ac12a5d41c0618e57d554cc688f5592ae67ea14c93f1`
- `crates/sophia-protocol/src/geometry.rs`: `27d0e7443da4baf8421de38ac22c54cecee92ca23e41cc0e477abbbe6fa77d99`
- `crates/sophia-protocol/src/codec_error.rs`: `7696a32183331bff7fda29e3038f5e2e7ed016c151dcdb8203f5f62b43990adf`
- `crates/sophia-protocol/src/packets/wm.rs`: `9f6b772bd62612bbdb6a5ea010dcdb6a5e48b3d472f22411ffae693f9083a495`
- `crates/sophia-protocol/src/packets/surface/layout.rs`: `432963d16cb3011e3e72067dc9506103cd2e562de432e14b7460657268716f5f`
- `crates/sophia-protocol/src/packets/surface/authority_surface.rs`: `99b5d25ec3a430749ab224e71a69a3d37d983125e6f564678051873bc4b9d96e`
- `crates/sophia-protocol/src/packets/translation.rs`: `8c7c11c86a36b58a79513ae820e4d8d785c6d2ffaba80082c665506ce0ff92d5`

The eleven integration test files and two shared fixtures come from
`crates/sophia-protocol/tests` at that revision. Imports name this crate and
the golden corpus path points to the SDK spec copy. The 71 checks cover
envelopes, capability gates, controls, complete arrays, reserved fields,
truncation, presentation, lifecycle, profiles and shared golden rows.

The KDL, API documentation, file documentation and golden corpus were compared
byte for byte with the C SDK at `997f22d`. They match its contract pins.
Generated `wm_rows.rs` is preserved byte for byte; its generator remains the
Sophia source owner. This crate adds no session state machine or authority.

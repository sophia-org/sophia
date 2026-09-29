# Shared protocol generation

Run `cargo run --offline -p sophia-policy-protocol-gen -- --check` to check
committed outputs, or omit `--check` to regenerate them.

The inputs are:

- `protocol/sophia-wm-files-v1.kdl`: neutral Rust row codecs, the shared record
  corpus and SMT arithmetic facts. Extension rows contribute samples and facts;
  their typed codecs remain with their semantic owners.
- `protocol/sophia-output-v1.kdl`: output wire documentation and corpora.
- `protocol/sophia-control-v1.kdl`: control wire documentation and corpora.

WM and shell socket schemas are no longer inputs. Their frozen C bindings,
schemas and corpora remain only where required by the current SDK pins.
The SDK release and re-vendoring step owns deletion of those compatibility
artifacts. This tool remains for the file rows and the separate output/control
roles; removing a role does not remove another role's generation checks.

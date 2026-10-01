# Shared protocol generation

Run `cargo run --offline -p sophia-policy-protocol-gen -- --check` to check
committed outputs, or omit `--check` to regenerate them.

The inputs are:

- `protocol/sophia-wm-files-v1.kdl`: neutral Rust row codecs, the shared record
  corpus and SMT arithmetic facts. Extension rows contribute samples and facts;
  their typed codecs remain with their semantic owners.
- `protocol/sophia-control-v1.kdl`: control wire documentation and corpora.

WM, shell and output socket schemas are retired. This tool retains WM file rows
and the separate control role; removing output IPC does not remove control
generation checks. Output files have native record conformance in
`sophia-protocol` and the public C SDK.

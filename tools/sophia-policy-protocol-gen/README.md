# Shared protocol generation

Run `cargo run --offline -p sophia-policy-protocol-gen -- --check` to check
committed outputs, or omit `--check` to regenerate them.

The input is `protocol/sophia-wm-files-v1.kdl`: neutral Rust row codecs, the
shared record corpus and SMT arithmetic facts. Extension rows contribute
samples and facts; their typed codecs remain with their semantic owners.

WM, shell, output and control socket schemas are retired. This tool retains WM
file rows. Output and administrative files have native record conformance in
`sophia-protocol` and the public C SDK.

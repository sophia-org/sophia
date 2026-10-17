# Sophia desktop SDK for Rust

Read README.md and PROVENANCE.md before changing the library. Sophia owns the
contracts; use their pinned copies in `spec/`. Keep generic 9P transport separate
from role semantics and keep tests outside production `src` files.

## C and Rust SDK parity (required)

Keep this SDK aligned with `sophia-org/sophia-desktop-sdk-c`. Public role
revisions, operations, events, errors, custody, retry, cancellation, revocation,
recovery and event-loop capabilities must have equivalent behavior. Document
ownership and lifetimes in both; idiomatic API names and storage models may
differ. Internal optimizations need no literal port when the other SDK has no
equivalent cost or defect.

Review every public change against both SDKs. Update affected implementations
and coverage in the same change series, run matching independent conformance
scenarios, and record exact revisions before Sophia updates its verified pins.
Never patch a vendored snapshot directly. Existing gaps remain tracked by
Sophia t333; record any blocked counterpart and its release consequences, and
do not claim parity or close that gate until both sides have evidence. Keep
native implementations independent: Rust callers use this SDK directly, and
foreign-language callers can bind to C without requiring Rust or Sophia.

## Build and commit discipline

Run Cargo through `cargo-slot` using the repository's slot pool. Never create a
task-specific target directory. Sign commits, preserve copyright and license
text, and omit coauthor trailers. Never reset or restart gpg-agent.

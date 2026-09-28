# Desktop SDK coverage

## Required boundary

The C SDK and Rust SDK remain independent repositories, one per language.
Each must cover the complete public WM, shell, output and admin/control surface
previously served by IPC. Modules and library archives inside a repository do
not create additional SDKs. Hagia's Nim bindings call the C SDK.

All roles use ordinary 9P2000.L operations and error numbers. Sophia defines
the admitted files and their typed contents, not a private 9P dialect. Role
authorization, semantic acceptance and presentation remain with the server's
owners. Sharing transport does not merge roles or grant authority across them.

No product implements its own 9P pipeline, role envelopes or submission custody.
Clients retain their own policy and UI logic. No missing file operation falls
back to IPC. Former descriptor-based operations require a reviewed file or
protected-grant contract, rather than a hidden ancillary-data channel.

## Release 0.1.0

| Role | File contract | Client status |
| --- | --- | --- |
| Shell: bar, native launcher, catalog/dock, indicators, content | Pinned API 1, revisions 6–8 | Implemented; scope and evidence in README and tests |
| WM: negotiation, profile handoff, configuration, snapshots, cycles, projections, session operations, presentation receipts | Sophia WM file API 1 | Implemented: literal/scripted tests, Sophia production WM export gate (`c4e17899e`) and Hagia thin bindings (`b3d8496`) |
| Output authority | Separate role; the pinned WM contract still advertises `output_transport=current_ipc` | File contract and SDK client gap |
| Admin/control: the public control operations and their results | Existing control schema does not establish a 9P file contract | File contract and SDK client gap |

This table does not claim that a transport codec provides a session client, or
that a unit test proves operation against a live Session. `compatibility.json`
continues to describe implemented support until the relevant gates pass.

## Parity gate

For each role, inventory every former IPC request, response, event, capability,
resource grant and terminal outcome against a named file-contract operation.
The inventory must include negative behavior: refusal, backpressure, stale
epochs, revocation, disconnect, unknown custody and resource retirement.

A role is complete only after its authoritative contract, server export, C and
Rust client modules, literal codec tests and real-export conformance tests agree.
Migration must not silently drop an operation or report socket tests as file
coverage. Removed product IPC tests are retired with that wire; their required
behavior is accounted for by the replacement file tests.

## Product pins

Products vendor or otherwise bind an exact SDK source revision and tree. Pins
are per product: adding WM support for Hagia does not change Bemenu's existing
SDK pin. Builds verify the vendor manifest and record its digest and revision.
There is no ambient sibling SDK checkout in the build inputs.

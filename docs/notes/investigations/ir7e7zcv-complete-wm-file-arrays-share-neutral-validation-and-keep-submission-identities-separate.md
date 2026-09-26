---
id: ir7e7zcv
date: 2026-09-25
kind: investigation
status: investigating
tags: [investigation]
---
# Complete WM file arrays share neutral validation and keep submission identities separate

## Question

Can complete Snapshot, Projection and Configuration file bodies reuse the
neutral row owner without transporting old Begin/Chunk/End frames or conflating
submission custody with policy transaction identity?

## Evidence

The source checkpoint follows signed `ce5dd51a8` in `sophia-overview` on
`protocol/9p-wm-contract`. It adds `wm_files/arrays.rs`, passive records and
external `tests/wm_file_arrays.rs`; the KDL schema pins all three prefixes.
Evidence is in `.artifacts/t249-arrays`. Builds were device-hidden, nice 19,
jobs 2, on the exclusive local `.artifacts/t249-target`.

## Finding and resolution

The file codec checks exact kind and common epoch, reserved fields, section
shape/count/aggregate bounds and selected section capabilities. Complete rows
then enter the same neutral conversion used by the legacy wrapper. Submission
81 and domain transaction 11 deliberately differ in the projection fixture.
The public API returns domain values; the file owner retains the envelope's
submission and admitted-epoch correlation.

Snapshot encoding omits unselected extensions. Candidate encoding and decoding
refuse unnegotiated sections at the codec boundary. The direct legacy codec
lacks that selected-set parameter; its runtime separately checks indicators,
tab/translation groups and presentation. This is not evidence that the complete
legacy IPC path admits those unnegotiated extensions. Session still owns capabilities within row content, output
coverage, admission and settlement. A snapshot's active output must occur in
its output section. Projection and Snapshot cannot omit all output sections.
Colours use `0x00RRGGBB`, not the legacy scalar frame's alpha byte.

The first focused run was 8/1 because the new action-without-bindings fixture
still declared a modal keyboard output. The existing presentation validator
correctly refused it. Clearing that modal field fixed the fixture; production
validation was not weakened. The initial log is retained.

## Validation and remaining work

Nine focused controls pass, including every snapshot/projection extension,
unnegotiated capabilities, exact class/kind/epoch, reserved colours and fields,
every truncation, missing output sections, and a complete 1024-instance array
larger than the 9P msize. The unchanged legacy baseline frame fixture remains
byte-equal. Full protocol: 210 passed, zero failed. Strict all-target protocol
Clippy, formatting, diff and a freshly compiled local xtask layout pass.

Three compiled mutations fail at their intended assertions: capability checks
bypassed, mandatory output check removed, and snapshot membership check removed.
All are restored before the final suite. No Session, Engine, live installation,
hardware or default transport changed. Scalar bodies, independent Nim body
corpus and the real Session/export pairing remain t249/h006 work; this is not
WM migration acceptance or a performance result.

## Scalar controls

The follow-up joins the shared scalar semantic owner `59eb5f34` as signed
`627fe79ba`, then defines complete Cycle, Dirty, SessionOperation, their outcomes,
PresentationReceipt and Submitted bodies. Shared helpers now live in
`wm_files/payload.rs`; array behavior is unchanged. The KDL states the new file
cause numbering, exact lengths and signed geometry fields. All typed decoders
retain strict neutral checks, separate epoch/submission/domain identities and
capability requirements. Submitted's checked body helper lets the journal
supply its actual sequence without inventing an event header.

Ten focused controls pass, covering every cause/outcome, exact literal offsets,
capabilities, strict targets, each cycle truncation, fixed-body lengths and
reserved fields. The first compile caught width/height typed as unsigned when
Sophia Rect uses signed fields; the source and schema were corrected together.
That failed log remains in `.artifacts/t249-controls`. Three compiled mutations
remove strict request validation, epoch binding and fixed-body length checks;
each fails its named control. Restored full protocol: 232/0; strict protocol
all-target Clippy, fresh-root layout, fmt and diff pass.

This remains protocol evidence. Negotiation/profile bodies, independent Nim
file bodies, and the complete admitted Session/Hagia roundtrip are not yet
proved by it. The separate supplied-stream custody checkpoint does not turn
these codec tests into semantic settlement or physical presentation evidence.

## Connections

- [WM file contract](../../sophia-wm-files.md) owns byte and custody semantics.
- [Migration plan](../plans/80blhke8-migrate-the-hagia-wm-role-to-admitted-9p2000-l-files.md)
  owns t247/t248/t249 scope and joined acceptance.
- [Neutral rows](e4tqd2ec-neutral-wm-records-preserve-legacy-framing-and-bound-complete-sections.md)
  records the unchanged legacy boundary and independent baseline fixture.

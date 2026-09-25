---
id: 5xr74jmt
date: 2026-09-25
kind: investigation
status: implemented
tags: [session, validation]
---
# QEMU recovery fixtures must come from a current guest run

## Trigger

t160's retained success fixture predates both the input poller's schema-4
readiness and the independent guard's schema-2 readiness. On `97a6b6f6`,
`tools/check_atomic_scanout_verifiers.sh` exits 1 at that supposed success case:
"QEMU emergency recovery evidence is missing physical input readiness".
Later verifier cases do not run.

## Guest evidence, 2026-09-25

The unchanged emergency-recovery harness ran against production source
`97a6b6f6b5b0bcb286f65a7c602f79ca13399a2b`. The binary was built from
`33355234dc660c45fadd8322712f29a3f139d135`; the production files and guest scripts
are identical between those commits. A private copy had debug sections stripped.

The guest used kernel `6.18.52_1`, its retained dracut base image, and a compressed
newc overlay containing that Sophia binary and the current guest init script.
An identity-only init wrapper printed the actual guest binary and script hashes
before executing the original init as PID 1. The guest reported:

- Sophia SHA256 `fe78b4f63717556855b3858162dc9d9fe69f57e5ceedc1538fa248bf16f4b1fb`.
- Guest init SHA256 `f8b665684ea04e5b126f11c480b874c8fc15c4da92bca96651d3baf47e3a309c`.
- Guard schema 2: six devices, three keyboards.
- Poller schema 4: six active devices, three keyboards and three pointers.
- QMP arming chord, guard armed, committed input focus, QMP trigger chord,
  guard triggered and Sophia emergency exit.
- Session and guard exit status 0, clean frontend/client cleanup, no native
  in-flight or cleanup obligation, and QEMU exit 0.

QEMU ran in a private process/network/IPC namespace with private `/dev`, `/tmp`
and `/run`. Only `/dev/kvm` was passed through. Graphics used software virtio
devices, input used virtual devices, and display/control used private Unix
sockets. The whole host command had a 180-second timeout. No host DRM, input,
VT or installed-session access was used. This is guest recovery evidence, not
physical installed-session acceptance.

## Fixture repair and validation

The success fixture is the guest capture copied byte-for-byte. The negative
fixture is the same capture with exactly the independent guard's triggered
record removed. It now refuses specifically for the missing guard trigger,
not an unrelated old readiness schema. No success record was fabricated and
the verifier was not weakened.

The unchanged guest verifier passes the new capture. The complete
`tools/check_atomic_scanout_verifiers.sh` passes, including every case after
the previously failing recovery fixture. `git diff --check` passes.

Raw logs, kernel, complete overlaid image, source/binary identities, guest
scripts and checksums are retained at
`~/.local/state/sophia/development-evidence/t160-97a6b6f6`.

The distinction between a virtual recovery rehearsal and physical acceptance
is preserved in [the input acceptance investigation](wq3n8fkz-which-physical-acceptance-rows-a-virtual-device-could-drive.md).

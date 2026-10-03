---
id: hhbejm8k
date: 2026-10-03
kind: adr
status: proposed
tags: [adr, security, authentication, 9p, plan9]
---
# Port 9front factotum to Rust as sophia-factotum

## Context

The [lock ADR](w0seozxx-session-owns-lock-state-and-authentication-lock-providers-only-render.md)
makes a Session authenticator the only unlock authority and names it after Plan 9's
factotum. Its first draft scoped that authenticator to PAM verification alone.
niltempus asked for a real factotum instead, built right the first time, in case
[t275](../investigations/kcfh2hdg-adopting-the-plan-9-namespace-model-in-sophia.md)
leads to authenticated 9P exports.

9front's factotum (`sys/src/cmd/auth/factotum`, about 7,200 lines of C, MIT) is a
user-level file system acting as a user's authentication agent. Opening `rpc`
gives a private conversation driven by `start`, `read`, `write`, `authinfo` and
`attr`; `ctl` adds and removes keys, never returning `!`-prefixed secret
attributes; `proto`, `confirm`, `needkey` and `log` complete the interface. Keys
are attribute tuples such as `proto=dp9ik dom=… user=… !password=…`. Protocol
modules share one interface (init, addkey, closekey, read, write, close), so the
agent is independent of any particular protocol.

Factotum depends on services that are not factotum. dp9ik and p9sk1 need an
auth server (`authsrv`, `keyfs`), which alone holds both parties' keys. Keys
persist through secstore. Ticket and authenticator formats, `passtokey` and the
authpak exchange live in libauthsrv. dp9ik uses SPAKE2-EE over Ed448 with decaf
and Elligator2 (written for 9front's `mpc`), HKDF-SHA256, ChaCha20-Poly1305
ticket encryption and PBKDF2-HMAC-SHA1 with 9001 iterations. Plan 9 verifies a
user's password by running `proto=dp9ik role=login` through factotum, which asks
the auth server.

Sophia's 9P core refuses `Tauth` today: `Tattach` requires `afid` to be `NOFID`,
and admission is by socket and peer credentials.

## Decision

Port 9front's factotum to Rust as the in-tree crate `crates/sophia-factotum`
(library and agent binary), byte-compatible with 9front on the wire.

- **Agent.** A separate, supervised, per-session process that is not dumpable,
  locks its memory and never writes cores. Session starts it before admitting
  clients. Secrets live only in it; `ctl` reads hide secret attributes.
- **File interface.** `rpc`, `ctl`, `proto`, `confirm`, `needkey` and `log`,
  served by `sophia-9p` with factotum's verbs, replies and phases. Admission is by
  socket, pidfd and same UID until t275 supplies namespace recipes. Lock providers
  and ordinary roles never receive it.
- **Protocols in the first release.** `pam` (new, Linux): `role=login` verifies a
  local user and password through PAM in an executed helper, so a PAM module can
  neither share the agent's address space nor block it. `pass`, as in 9front.
  `p9any` and `dp9ik` in client and server roles, byte-compatible with 9front.
  Other 9front protocols are not ported until a consumer needs them; legacy
  `p9sk1` (DES) is excluded.
- **Unlock is a factotum client.** Session opens `rpc`, starts
  `proto=pam role=login`, writes the user and the secret from its locked buffer and
  accepts only an `ok` whose conversation belongs to the current lock epoch and
  attempt. When a 9P auth domain exists, the same exchange can use
  `proto=dp9ik role=login`.
- **Shared auth-server code.** Ticket, authenticator and key formats,
  `passtokey`, `form1` and authpak are ported into a small crate,
  `sophia-libauthsrv`, that factotum and the auth server share.
- **Auth server outside Sophia.** authsrv and keyfs (and later secstore) are
  network identity infrastructure for an auth domain, which can include 9front
  machines that never run Sophia. They are ported in their own repository,
  `sophia-org/authsrv`, and share `sophia-libauthsrv`. Sophia's factotum only dials an auth server, whether
  that port or a real 9front one.
- **Interoperability evidence.** An independent C oracle built from 9front's own
  libauthsrv and libsec sources, including the C that `mpc` generates, produces
  vectors and runs live exchanges against the Rust modules.
- **Exports.** Wiring `Tauth` and per-export factotum server conversations into
  `sophia-9p` waits for t275 to decide whether exports are authenticated across
  users or machines. Local admission by socket and pidfd remains stronger than a
  password-derived ticket on one host.
- **Prompts.** `confirm` and `needkey` need trusted UI that cannot be spoofed.
  Until the lock's secure input path is generalized into a secure prompt, keys are
  added through `ctl` and an unanswered `needkey` fails.

## Alternatives

**PAM-only authenticator.** The first lock draft. Smaller, but a later dp9ik
would reshape the agent instead of adding a module.

**Same design with a Sophia-only wire.** Less to verify, but no interoperability
with 9front auth and file servers. Not selected.

**Auth server in Sophia's tree.** One tree to keep in step, but Sophia would carry
a networked key server that none of its processes need. Not selected.

**Standalone factotum repository.** A pinned agent outside Sophia's gates, while
unlock correctness depends on it. Not selected; a split stays possible if
non-Sophia consumers appear.

## Consequences

Sophia gains cryptographic code for the first time. niltempus decided on
2026-10-03 to take the primitives (SHA-256, HMAC, HKDF, PBKDF2-HMAC-SHA1,
ChaCha20-Poly1305) from audited RustCrypto crates and to port only authpak's
curve code: the Ed448 field arithmetic, decaf, Elligator2 and SPAKE2-EE that
9front writes for `mpc`. The C oracle checks the bytes of both.

Ported files carry 9front's MIT notice in their headers and an entry in
`THIRD-PARTY-NOTICES.md`.

Unlock depends on the agent: t293 delivers the factotum core and `pam` before
t292 can unlock anything but its test seam.

Persistent key storage (secstore) and secure prompts remain open.

## Acceptance and connections

Proposed 2026-10-03. niltempus chose byte compatibility with 9front, a ported auth
server in its own repository, `pam`, `pass`, `p9any` and `dp9ik` in the first
release, and the in-tree crate. Tasks are t293 (agent core and `pam`) and t298
(`pass`, `p9any`, `dp9ik`, `sophia-libauthsrv` and the C oracle) in the
[lock plan](../plans/8jcykhdc-secure-session-lock-authority-and-lock-provider-role.md).
Reference source: 9front `sys/src/cmd/auth/factotum`, `sys/src/libauthsrv`,
`sys/src/libauth`, `sys/src/libsec/port`, man pages factotum(4) and authsrv(6).

---
id: torntn5a
date: 2026-10-03
kind: plan
tags: [plan, milestone]
---
# Nix-built processes in protection domains

## Scope and exit

niltempus is evaluating a desktop built by Nix (niltempus n002: the plan
`c7g8cnd5` and the phase A results in the niltempus repository). A binary
built by Nix loads its interpreter and libraries from `/nix/store`. Sophia's
protection domains bind only `/usr` and `/etc/ld.so.cache`, so such a binary
cannot start in any role. t301 is phase B of n002, and is generic: it names no
product and no client.

Exit:
- Every protection domain sees a host's `/nix/store` read-only, through its
  own read-only binding, when the store is a real directory. Without a store,
  the argument list is byte-identical to today's.
- No grant may replace or shadow the store: a destination at, above or
  inside it is refused at launch.
- The Bubblewrap executable comes from trusted session configuration through
  `ProtectionDomainSpec::bubblewrap_path`, default `/usr/bin/bwrap`. It must
  be an absolute path to an executable file, and the version check and launch
  refusals stay unchanged.
- Role, device and network confinement are unchanged.
- The full Sophia gate and the layout gate pass, and Codex reviews before
  merge. There is no live change.

## Task details

### t301

Two reviewable commits:
1. **Runtime** (`sophia-runtime` `supervisor/protection.rs`):
   - the store binding, emitted after the fixed layout and before any grant;
   - the shadowing refusal;
   - store-resident programs need no separate binding;
   - the configured Bubblewrap must be an absolute executable file.

   Unit tests drive the argument builder with the store present and absent.
   An opt-in smoke (`SOPHIA_RUN_PROTECTION_DOMAIN_SMOKE`) launches every role
   and proves the store read-only by its mount flags, that a write fails, and
   that a Nix-built executable from the host store runs.
2. **Session**: the trusted `session run --bubblewrap=/absolute/path` option
   names the Bubblewrap executable. A named one is validated at argument
   parsing; the default keeps the launch-time checks. It reaches every
   live-session domain through the existing builder method: the WM, the
   output authority, shell components, the legacy shell, the metadata broker
   and the lock provider. The standalone GPU content proof and the CLI
   broker smoke start no session, so they keep the default. A structural test
   names every domain construction in sophia-session and each forward of the
   session's choice.

Bounded mutants (each removing the binding, the read-only flag, the
shadowing refusal or the executable validation) must each fail a named test.

Not in t301: graphics drivers for Nix builds (n002 phase D), the niltempus
release kind (phase C), and an FHS root for running Sophia's workspace tests
inside Nix's sandbox.

## Connections

- niltempus n002 plan `c7g8cnd5` and its phase A results (niltempus
  repository): the 40 workspace tests that fail with protection-domain spawns
  under Nix are this task's scope.
- [Secure session lock](8jcykhdc-secure-session-lock-authority-and-lock-provider-role.md):
  the lock provider is one of the roles exercised.

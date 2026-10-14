# Adopt a Session namespace

Status: t318 candidate, qualified in isolated tests; signed integration and
installed acceptance are separate. This guide applies to independently built
tools. Ordinary installed applications use Session's configured application
assignments and need no manual socket or authentication setup to launch.

## Launch under approved policy

An administrator selects the recipe. For a developer tool, the core policy can
enable an observer recipe:

```kdl
session {
    namespace-policy {
        recipe "tools" profile="observer" developer-launch=#true
    }
}
```

The developer interface also requires explicit host-admin access. With both
enabled, Session logs its private launch socket. Start an independent program
using the [developer launcher](developer-launch-files.md):

```sh
sophia launch --socket /absolute/session/launch.sock --recipe tools -- /absolute/tool --flag
```

The recipe grants visibility and bounded participation; it does not approve a
capture or confer administration or input injection. Starting the program
directly from a shell does not provision its launch capability. A Started
result means process readiness, not a successful role operation or a window.

## Capture one output

For output capture, the observer recipe also needs explicit trusted approval:

```kdl
recipe "capture" profile="observer" developer-launch=#true {
    capture-output "pci-0000:03:00.0/DP-1"
}
```

Launch the installed CLI through that recipe, using the output ID from the
admitted output view:

```sh
sophia launch --socket /absolute/session/launch.sock --recipe capture -- /absolute/sophia capture OUTPUT_ID
```

The child reports the output incarnation, presentation generation, dimensions
and pixel checksum. `--output NEW_FILE.pam` also writes an image when its
namespace explicitly admits a writable destination; the standard observer
recipe does not grant access to the host's home directory. The CLI pipelines
bounded reads under one expiring grant. Lock, revocation or output replacement
can cancel a read; it does not retry with a different grant. This path remains
under release and physical qualification.

## Discover, authenticate, then use the role

```text
Session policy -> supervised program + private launch capability
                            |
                   SOPHIA_NAMESPACE_DIR
                            |
                  pin directory + discovery
                            |
             check endpoint protocol/interface/version
                            |
                connect the pinned endpoint inode
                            |
          own Tauth challenge -> private launch relay
                            |
                   attach with verified afid
                            |
                  use returned role root
                            |
          retained handles stay with that service/grant
```

1. Pin the directory supplied in `SOPHIA_NAMESPACE_DIR`. The Rust SDK exposes
   `sophia_namespace_client::Namespace::from_env()`; the C SDK exposes
   `sophia_ns_open()`. Keep the namespace object or directory descriptor for
   later lookups in that view. The environment path is a location, not a
   credential.
2. Read discovery and select a declared endpoint. Check protocol, interface and
   version before connecting. Availability is descriptive and can change; it
   does not replace the result of an operation. The
   [discovery contract](namespace-discovery.md) defines bounds and identities.
3. Use Rust `Namespace::connect(path)` or C `sophia_ns_connect()`. These pin the
   endpoint inode, reject symlink traversal and support logical paths longer
   than a Unix socket address. Do not reconstruct a mutable pathname yourself.
4. For 9P, transfer the role stream and the exclusively owned launch relay to
   Rust `sophia_launch_client::Client::over`, or the C launch client. Adopt the
   provisioned descriptor through the platform bootstrap described in the
   [launch authentication contract](factotum-launch-authentication.md). Never
   treat an arbitrary descriptor number as authority. Relay only the challenge
   from this role connection's own afid.
5. Drive authentication until ready, respecting its deadline. Use the returned
   authenticated role pipeline and root directly. Do not perform a second plain
   attach. Read the role's API and use its documented files and codecs.
6. For another endpoint, transfer the quiescent relay using Rust
   `Client::over_relay` or C `sophia_la_next_endpoint`. Keep earlier role roots;
   never drive two authentication conversations on the relay concurrently.

X11 endpoints continue to use their X authority's DISPLAY/XAUTHORITY setup;
discovery does not turn X11 into a 9P role or bypass X admission.

## Generic observer example and failure handling

The shipped observer view names its inspection endpoint `roles/status`. A tool
checks that endpoint's advertised inspection contract, authenticates with the
inspection audience, reads `api`, then reads the role's `snapshot`. Its returned
bytes are a snapshot, not permission to control the desktop. The generic
[joined observer peer](../crates/sophia-session/tests/support/namespace_inspection_dispatch.rs)
demonstrates the independent Rust SDK discovery/authentication sequence and
retained/waiting-read revocation. Its test bootstrap supplies the launch relay
through stdin; that fixture convention is not an application ABI. The independent
[Rust SDK](../vendor/rust-desktop-sdk/source/README.md) and
[C SDK](../vendor/c-desktop-sdk/source/README.md) document their public APIs.

A replacement view affects new lookups, not the service or permission of an
existing fid. Keep service identity, generation and permission separate from
the visible name. ESTALE or lost admission ends the affected operation: do not
silently reconnect to a same-named service or retry a possibly submitted side
effect. An unrelated grant can remain usable after one endpoint is revoked.
Authentication failure has no unauthenticated fallback.

Capture requests use the separate portal recipient contract and still require
approval and a live target generation. Renderer capture bytes are t319 work;
this example neither captures pixels nor injects input. The native 9P
application frontend is also a separate deliverable.

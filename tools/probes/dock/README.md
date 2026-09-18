# Three-component dock smoke

From tty4, after ending the existing graphical session:

```sh
lom-test dock
```

This is a bounded attended smoke, not a normal-session installer. It builds the
clean, signed Sophia, Lom, Provlita, Bemenu and Hagia sources, records revisions,
binary/config hashes, and runs the existing protected Lom GPU proof before
graphics takeover. Provlita shares Lom's GPU adapter; that proof does not render
Provlita's scene. Development Provlita dependencies must resolve to the exact
selected sibling Sophia and Lom checkouts. No installed configuration is changed.

The generated profile admits three independent processes: Lom bar (revision 6,
direct GPU, top 24), Bemenu launcher (revision 7, GPU denied), and Provlita dock
(revision 8, direct GPU, bottom 64). It preserves the selected WM's policy and
keybindings, requires its application-launcher binding, and starts no application
automatically. The explicit test catalog includes `registered:terminal`; example
Browser/Files tiles remain unavailable when their identities are absent.

The session lasts 90 seconds, with the existing 110-second recovery watchdog.
On each monitor click Terminal on the dock once, confirm it stays open, and type
`exit` followed by Enter; then open Bemenu
with the WM binding (Super+Space in the operator profile), search for terminal,
launch it once and exit its shell the same way. Reopen/dismiss Bemenu to check query reset. Check
workspace switching and moving clocks on both bars throughout. Let the session
end automatically. Record visual placement, focus behavior and any flashing.

The Rust `cargo xtask dock verify HOST_LOG` reader requires three distinct
role/slot/grant identities, the exact per-role revision/GPU policy, matching
two-output presentation, actual Session-adopted launches from menu and dock on
each output, matching successful child exits, a zero protocol-refusal tally,
and quiescent component shutdown. It rejects restarts, failed
service, malformed identities and missing launch/presentation evidence. A clean
native exit and TTY/keyd restoration are separate launcher checks. No client ACK
substitutes for an actual launched process. Source/build and transcript evidence
are retained under `.artifacts/lom-panel-native/<timestamp>`.

VT switching is a separate attended lifecycle check; do not mix it into this
automatic-exit smoke. Record release/resume and retained-owner evidence separately.

Passing this transcript is not visual acceptance, measured latency, restart
isolation, GPU quota enforcement or complete t005/t006 acceptance. Unexpected
catalog/topology replacement may still refuse a client and fail this smoke; it
must not be reclassified as success. The resource/control tests use real private
sockets and owners, but substitute renderer/native completion and do not prove
hardware behavior. No automated check invokes this physical command.

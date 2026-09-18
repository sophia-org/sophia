# Triad Port Ledger (External Gate)

**Role:** pointer to a freeze gate that lives outside this repository.

`todo.md`'s `sophia_wm_v1` freeze item has two conditions. The first — that the
retained Triad behavior port is complete — is defined by a ledger that lives in
the **Hagia** repository, not here:

    docs/triad-port-ledger.md        (in the Hagia checkout)

This file exists so that a reader of `todo.md` can find that gate. It is a
pointer and a summary, not a copy. **The Hagia file is authoritative.** Do not
resolve a disagreement between them by editing the summary below; re-read the
ledger.

## Identity

- Reviewed Triad source baseline: `fb8fb27ec294e0fe2361375de0b2fa8c08be0ca9`.
  A later Triad change is considered separately and does not move this baseline.
- Default checkout location: sibling of this repository, `../hagia`. The
  cross-repository conformance gate resolves it through `SOPHIA_HAGIA_ROOT` and
  runs Hagia's suite with `SOPHIA_ROOT` pointing back here.

## Completion Rule

Interface major 1, wire revision 3 is stable. The gate closed on 2026-08-26
after every retained row was complete. Excluded rows stay visible as
post-freeze product work and carry a written rationale; they are not silently
treated as implemented.

An exclusion requires a written architectural or product rationale. "Not yet
implemented" is not an exclusion.

## Row States

Twenty-eight classified rows across four authority tables:

| Table | Rows | Complete | Partial | Open | Excluded |
| --- | --- | --- | --- | --- | --- |
| Spatial Policy — Hagia | 12 | 11 | 0 | 0 | 1 |
| Visible Desktop — Hagia Shell | 5 | 2 | 0 | 0 | 3 |
| Session And Dedicated Sophia Authorities | 7 | 5 | 0 | 0 | 2 |
| Brokers And Portals | 4 | 3 | 0 | 0 | 1 |

Totals: 21 complete, 0 partial, 0 open, and 7 excluded. The checked-in Hagia
daily-driver profile, rather than every binding in Triad's historical default,
defines the freeze surface. Trusted one-shot launch placement and frame-fed
output activation are implemented and proven. Frame-fed physical archive `0001`
binds Sophia `870ba46ae231081220b982ecc3a5a95517df7a90` and Hagia
`a83c8fa022a4ceff5d8b96a01c46052bbd8ba64a`; the shared reconnect/restart
corpus and immutable archived revision-3 client also pass.

Shell surface policy left Hagia on 2026-09-04 for `sophia-org/narthex`, a
separate client with a strictly smaller capability. The rows below record the
state at the 2026-08-26 freeze and are not restated; the work happened in Hagia
at that time and the archives bind those commits.

`hagia-shell` now exists as source: Hagia commits `216fb87`, `3795dce`,
`c33a1f4`, and `a76528f` add the experimental client, the live shell service,
the protected switcher, and unlabeled descriptor decoding. Signed archive
`0006` proves the retained generic switcher; signed archive `0007` separately
proves coherent work-area reservation and reconnect. MRU policy, previews,
icons, and persistent Tier-1 panels are explicit post-freeze work.

## Consequences For Work In This Repository

- **API v7 is removed.** The retained output row and revision-3 freeze
  conditions closed first. The client-hosted socket, codecs, demo server,
  Engine transport, and selectable configuration value are gone. Shortcut
  matching remains protocol-neutral in Engine, while workspace models belong
  to native policy clients. The public Hagia and archived-client gates remain
  the regression boundary.
- **Several items filed under `todo.md`'s Post-Promotion Capability Roadmap are
  already completed the freeze path.** Protection-domain enforcement, the
  redacted status feed, explicit-grab reduction, and frame-fed atomic
  multi-output apply/rollback are complete; lock, watched reload, and a
  separate output-power authority are explicitly post-freeze.
- **Wire-layout risk is enumerated separately** in
  `docs/wm-v1-freeze-surface.md`, which classifies every ledger row by whether
  closing it can force a `sophia_wm_v1` layout change.

## Binding Capacity Is Not The Constraint

The ledger records the binding inventory carefully, and it is easy to misread.
Interface revision 3 admits **256** binding registrations. Hagia's compiled profile
contains 51 Sophia-owned chords resolved against its 66-entry action catalog,
while Triad's baseline default holds 132 key and 137 total physical bindings, of
which the semantic migrator emits 39 key plus 2 pointer bindings today. The
remaining bindings stay classified with explicit retained or excluded
dispositions; exclusion does not consume a WM registration slot.

The constraint is the **authority split**, not a slot count. Every accepted
binding must name its matching owner and behavior owner; every excluded binding
must remain visible in the migration report.

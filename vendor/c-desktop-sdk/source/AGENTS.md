# Sophia desktop SDK for C

Read README.md and PROVENANCE.md before changing the library.

- Keep the generic 9P client independent of shell records and IPC framing.
- This is one desktop SDK for every public WM, shell, output and admin/control
  role over standard 9P2000.L. Add role modules here, not separate SDKs or
  product-owned protocol clients. Track missing contracts and parity explicitly
  in COVERAGE.md; never fill a gap with IPC fallback or private 9P operations.
- File record layouts and value rules come from the pinned KDL in spec/.
  Report contract gaps to Sophia; do not infer rules from its implementation.
- Keep public values passive, memory bounded, and ownership explicit.
- Put tests in src/tests/. Keep live production-export harnesses in Sophia.
- Build and test at normal priority with the available CPUs; do not add nice or
  fixed job counts to build or test commands.
- Sign commits; never reset or restart gpg-agent. Do not add coauthor trailers.
- Preserve license and copyright text.

# Sophia's generic development and evidence entry points.
# Desktop installation, physical runners and comparisons live in niltempus.

_default:
    @just --list --unsorted

check:
    @cargo --quiet xtask check

check-layout:
    @cargo --quiet xtask check layout

check-profiles:
    @cargo --quiet xtask profile check

direct-scanout-archive run='':
    @cargo --quiet xtask conformance verify direct-scanout-archive "{{ run }}"

direct-scanout-verify log='':
    @cargo --quiet xtask conformance verify direct-scanout-standalone "{{ log }}"

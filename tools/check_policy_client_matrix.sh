#!/bin/sh
set -eu

# Client products gate their own behavior in their repositories. Sophia's
# matrix covers its export, reducers and independent SDK conformance peer.
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
exec sh "$root/tools/check_policy_protocol.sh"

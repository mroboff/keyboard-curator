#!/usr/bin/env bash
# Only kc-app may depend on the GUI framework. Every other crate must stay
# UI-independent so framework churn cannot reach the core logic.
set -euo pipefail

status=0
for manifest in crates/*/Cargo.toml; do
    crate=$(basename "$(dirname "$manifest")")
    [ "$crate" = "kc-app" ] && continue
    if cargo tree --locked -p "$crate" -e normal,build --prefix none | grep -Eq '^gpui'; then
        echo "error: $crate depends on GPUI; only kc-app may" >&2
        status=1
    fi
done
exit $status

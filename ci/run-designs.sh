#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 aesc silicon
#
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Run gdsfill over every reference design of one PDK and verify with gdscheck
# that the filled layout meets the foundry density rules.
#
#   ci/run-designs.sh <process>
#
# Environment:
#   REFERENCE_DESIGNS  checkout of aesc-silicon/reference-designs (default: ../reference-designs)
#   GDSFILL, GDSCHECK  binaries to use (default: target/release/gdsfill, gdscheck from PATH)
#   WORKDIR            scratch directory for layout copies (default: mktemp)
set -euo pipefail

process=${1:?usage: $0 <process>}
ref=${REFERENCE_DESIGNS:-$(dirname "$0")/../../reference-designs}
gdsfill=${GDSFILL:-$(dirname "$0")/../target/release/gdsfill}
gdscheck=${GDSCHECK:-gdscheck}
workdir=${WORKDIR:-$(mktemp -d)}

[[ -x $gdsfill ]] || gdsfill=gdsfill
command -v "$gdsfill" >/dev/null || { echo "gdsfill not found" >&2; exit 1; }
command -v "$gdscheck" >/dev/null || { echo "gdscheck not found" >&2; exit 1; }

# Print only the per-layer summary of `gdsfill density`; tile lines are too noisy for CI.
density_summary() {
    "$gdsfill" density --process "$process" "$1" | grep -E '^(GDS|Density area|Layer|  Overall)'
}

# gdscheck exits 0 even with violations, so parse its summary line.
density_clean() {
    local out
    out=$("$gdscheck" run --process "$process" --suite density --topcell "$1" --input "$2")
    echo "$out"
    grep -q '^DRC clean\.$' <<<"$out" && return 0
    [[ $out =~ ^([0-9]+)\ violation\(s\)(,\ ([0-9]+)\ waived)?: ]] || return 1
    [[ ${BASH_REMATCH[1]} -eq ${BASH_REMATCH[3]:-0} ]]
}

failed=()
count=0
while IFS=$'\t' read -r name _ path topcell; do
    count=$((count + 1))
    work=$workdir/$name.gds.gz
    cp "$path" "$work"
    echo "=== $name ($process, top cell $topcell) ==="
    if  echo "--- density before" && density_summary "$work" &&
        echo "--- erase"          && "$gdsfill" erase --process "$process" "$work" &&
        echo "--- fill"           && "$gdsfill" fill  --process "$process" "$work" &&
        echo "--- density after"  && density_summary "$work" &&
        echo "--- gdscheck density suite" && density_clean "$topcell" "$work"
    then
        echo "=== $name: PASS"
    else
        echo "=== $name: FAIL"
        failed+=("$name")
    fi
done < <("$ref/designs.py" --tool gdsfill --process "$process" --absolute)

echo
echo "$count design(s) run for $process, ${#failed[@]} failed${failed:+: ${failed[*]}}"
[[ ${#failed[@]} -eq 0 ]]

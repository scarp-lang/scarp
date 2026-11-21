#!/usr/bin/env sh
# Compare the candidate's Lifetime microbench with a pinned historical commit.
# The same benchmark source is used for both checkouts, even if the baseline
# predates the new benchmark. Requires a complete local Git history and Python 3.
set -eu

if [ "$#" -ne 1 ] || [ -z "$1" ]; then
    echo "Usage: sh scripts/benchmarks/compare-lifetime.sh BASE_COMMIT" >&2
    exit 2
fi

root=$(git rev-parse --show-toplevel)
base_ref=$1
tmp=$(mktemp -d)
cleanup() {
    git -C "$root" worktree remove --force "$tmp/baseline" >/dev/null 2>&1 || :
    rm -rf "$tmp"
}
trap cleanup EXIT HUP INT TERM

git -C "$root" worktree add --detach "$tmp/baseline" "$base_ref"
cp "$root/benches/lifetime.rs" "$tmp/baseline/benches/lifetime.rs"
if ! grep -q '^name = "lifetime"$' "$tmp/baseline/Cargo.toml"; then
    cat >> "$tmp/baseline/Cargo.toml" <<'BENCH'

[[bench]]
name = "lifetime"
harness = false
BENCH
fi

printf 'Baseline source: %s\n' "$(git -C "$tmp/baseline" rev-parse HEAD)"
printf 'Candidate source: %s\n' "$(git -C "$root" rev-parse HEAD)"
printf 'Bench source: candidate/benches/lifetime.rs (copied verbatim)\n'
printf 'Host: %s %s\n' "$(uname -s)" "$(uname -m)"
rustc --version

(cd "$tmp/baseline" && cargo bench --locked --offline --bench lifetime -- --quick) > "$tmp/baseline.csv"
(cd "$root" && cargo bench --locked --offline --bench lifetime -- --quick) > "$tmp/candidate.csv"

python3 - "$tmp/baseline.csv" "$tmp/candidate.csv" <<'PY'
import csv
import sys

def read(path):
    with open(path, newline="", encoding="utf-8") as handle:
        rows = list(csv.DictReader(handle))
    if not rows:
        raise SystemExit(f"No benchmark rows in {path}")
    return {row["workload"]: float(row["median_ns_per_operation"]) for row in rows}

baseline = read(sys.argv[1])
candidate = read(sys.argv[2])
if baseline.keys() != candidate.keys():
    raise SystemExit("Baseline and candidate benchmark workloads differ")
print("workload,baseline_ns,candidate_ns,delta_percent")
for name, before in baseline.items():
    after = candidate[name]
    if before <= 0:
        raise SystemExit(f"Invalid zero baseline for {name}")
    print(f"{name},{before:.3f},{after:.3f},{(after - before) / before * 100:.2f}")
print("Short-run measurements are noisy; repeat before claiming speed changes.")
PY

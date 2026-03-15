#!/usr/bin/env bash
set -uo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PARTS_DIR="$ROOT_DIR/parts"
RESULTS_DIR="$ROOT_DIR/results"
BINARY="$ROOT_DIR/target/release/svsim"

mkdir -p "$RESULTS_DIR"

dirs=()
while IFS= read -r dir; do
    if compgen -G "$dir/*.sv" > /dev/null && compgen -G "$dir/*.json" > /dev/null; then
        dirs+=("$dir")
    fi
done < <(find "$PARTS_DIR" -mindepth 1 -maxdepth 1 -type d | sort)

if [[ ${#dirs[@]} -eq 0 ]]; then
    echo "No runnable parts directories found under $PARTS_DIR" >&2
    exit 1
fi

echo "Building svsim CLI (release)..."
cargo build -q -p svsim-cli --release --manifest-path "$ROOT_DIR/Cargo.toml"
build_status=$?
if [[ $build_status -ne 0 ]]; then
    exit "$build_status"
fi

if [[ ! -x "$BINARY" ]]; then
    echo "Expected CLI binary at $BINARY after build" >&2
    exit 1
fi

status=0
aggregate_args=()

for dir in "${dirs[@]}"; do
    name="$(basename "$dir")"
    output="$RESULTS_DIR/svsim_parts_${name}.json"
    aggregate_args+=(--json-test-dir "$dir")

    echo "Running $dir"
    if "$BINARY" --json-test-dir "$dir" > "$output"; then
        echo "Wrote $output"
    else
        echo "Wrote $output (one or more suites failed)" >&2
        status=1
    fi
done

aggregate_output="$RESULTS_DIR/svsim_parts_all.json"
echo "Running aggregate corpus report"
if "$BINARY" "${aggregate_args[@]}" > "$aggregate_output"; then
    echo "Wrote $aggregate_output"
else
    echo "Wrote $aggregate_output (one or more suites failed)" >&2
    status=1
fi

exit "$status"

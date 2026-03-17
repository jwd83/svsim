#!/usr/bin/env bash
set -euo pipefail

if [[ $# -eq 0 ]]; then
    json_test_dirs=("parts/failing")
else
    json_test_dirs=("$@")
fi

tmp_stdout=$(mktemp)
tmp_stderr=$(mktemp)

cleanup() {
    rm -f "$tmp_stdout" "$tmp_stderr"
}

trap cleanup EXIT

cmd=(cargo run -q -p svsim-cli --)
for json_test_dir in "${json_test_dirs[@]}"; do
    cmd+=(--json-test-dir "$json_test_dir")
done

set +e
"${cmd[@]}" >"$tmp_stdout" 2>"$tmp_stderr"
status=$?
set -e

if [[ ! -s "$tmp_stdout" ]]; then
    if [[ -s "$tmp_stderr" ]]; then
        cat "$tmp_stderr" >&2
    fi
    exit "$status"
fi

python3 - "$tmp_stdout" "${json_test_dirs[@]}" <<'PY'
import json
import sys
from pathlib import Path


def normalize_directories(payload: dict) -> list[dict]:
    report = payload["report"]
    if "directories" in report:
        return report["directories"]
    return [{"directory": payload.get("directory", "<unknown>"), "report": report}]


def pluralize(count: int, singular: str, plural: str | None = None) -> str:
    if count == 1:
        return singular
    return plural or f"{singular}s"


payload = json.loads(Path(sys.argv[1]).read_text())
directories = normalize_directories(payload)

failed_suite_count = 0
failed_case_count = 0

for directory in directories:
    directory_name = directory["directory"]
    report = directory["report"]
    failed_suites = [suite for suite in report["suites"] if not suite["passed"]]

    print(f"{directory_name}: {len(failed_suites)}/{report['total']} failed suites")

    if not failed_suites:
        print("  all suites passed")
        print()
        continue

    for suite in failed_suites:
        failed_suite_count += 1
        print(f"  suite: {suite['source_path']}")
        print(f"  json:  {suite['json_path']}")
        if suite.get("top_module"):
            print(f"  top:   {suite['top_module']}")

        suite_error = suite.get("error")
        if suite_error:
            print(f"  error: {suite_error}")

        suite_report = suite.get("report")
        if suite_report:
            failed_cases = [case for case in suite_report["cases"] if not case["passed"]]
            for case in failed_cases:
                failed_case_count += 1
                description = case.get("description")
                steps = case["steps"]
                print(
                    f"  case:  {case['name']} ({steps} {pluralize(steps, 'step')})"
                )
                if description:
                    print(f"  note:  {description}")

                for failure in case["failures"]:
                    step = failure.get("step")
                    location = f"step {step}, " if step is not None else ""
                    actual = failure.get("actual")
                    actual_text = "missing" if actual is None else str(actual)
                    print(
                        "    "
                        f"{location}{failure['signal']}: "
                        f"expected {failure['expected']}, actual {actual_text}"
                    )

        print()

print(
    f"failed suites: {failed_suite_count}, "
    f"failed {pluralize(failed_case_count, 'case')}: {failed_case_count}"
)
PY

if [[ -s "$tmp_stderr" ]]; then
    cat "$tmp_stderr" >&2
fi

exit "$status"

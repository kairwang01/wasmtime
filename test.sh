#!/usr/bin/env bash
set -uo pipefail

ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
cd "$ROOT_DIR"

if [ "${1:-}" != "--output_path" ] || [ -z "${2:-}" ] || [ "$#" -ne 3 ]; then
  echo "usage: $0 --output_path <junit.xml> <base|new>" >&2
  exit 2
fi

OUTPUT_PATH="$2"
MODE="$3"
RUN_DIR="$(mktemp -d)"
trap 'rm -r "$RUN_DIR"' EXIT

STATUS=0
REPORTS=()
RUST_FEATURES="run,serve,cranelift,wat,pooling-allocator,component-model-async"
C_API_FEATURES="async,profiling,cache,threads,gc,cranelift,wat,pooling-allocator,component-model"
C_API_HEADER_DIR="target/c-api-config"
C_API_LIBRARY_DIR="target/debug"
PAGE_SIZE_1_RUST_TESTS=(
  page_size_1_pool_capacities_are_independent
  page_size_1_pool_capacity_is_exact_product
  page_size_1_pool_cli_options_round_trip
  page_size_1_pool_component_limits_are_independent
  page_size_1_pool_configuration
  page_size_1_pool_does_not_limit_host_created_memories
  page_size_1_pool_fallback_preserves_component_limit
  page_size_1_pool_falls_back_to_shared_capacity
  page_size_1_pool_metrics_include_both_pools
  page_size_1_pool_mixed_decommit_recycles_both_pools
  page_size_1_pool_preserves_module_memory_limit
  page_size_1_pool_purges_dropped_modules_from_both_pools
  page_size_1_pool_reuse_resets_memory
  page_size_1_pool_size_and_growth_limits_are_independent
  page_size_1_pool_toml_options_round_trip
  page_size_1_pool_validates_capacity_edges
)

export CARGO_BUILD_JOBS=4
export CARGO_INCREMENTAL=0
export CARGO_NET_OFFLINE=true
export CARGO_PROFILE_DEV_DEBUG=0
export CARGO_PROFILE_TEST_DEBUG=0
export CARGO_TERM_COLOR=never
export WASMTIME_TEST_NO_HOG_MEMORY=1

write_command_report() {
  local name="$1"
  local status="$2"
  local log="$3"
  local report="$4"
  python3 - "$name" "$status" "$log" "$report" <<'PY'
import pathlib
import sys
import xml.etree.ElementTree as ET

name, status_text, log_path, report_path = sys.argv[1:]
status = int(status_text)
root = ET.Element(
    "testsuites",
    name="wasmtime-selected-tests",
    tests="1",
    failures="1" if status else "0",
    errors="0",
    time="0",
)
suite = ET.SubElement(
    root,
    "testsuite",
    name=name,
    tests="1",
    failures="1" if status else "0",
    errors="0",
    skipped="0",
    time="0",
)
case = ET.SubElement(suite, "testcase", name=name, classname="command", time="0")
if status:
    failure = ET.SubElement(case, "failure", message=f"command exited with status {status}")
    failure.text = pathlib.Path(log_path).read_text(errors="replace")[-200_000:]
ET.ElementTree(root).write(report_path, encoding="utf-8", xml_declaration=True)
PY
}

write_cargo_report() {
  local name="$1"
  local status="$2"
  local log="$3"
  local report="$4"
  shift 4
  python3 - "$name" "$status" "$log" "$report" "$@" <<'PY'
import pathlib
import re
import sys
import xml.etree.ElementTree as ET

name, status_text, log_path, report_path, *expected_tests = sys.argv[1:]
status = int(status_text)
log = pathlib.Path(log_path).read_text(errors="replace")
pattern = re.compile(r"^test (.+) \.\.\. (ok|FAILED|ignored(?:,.*)?)$", re.MULTILINE)
results = pattern.findall(log)

if expected_tests:
    outcomes = dict(results)
    missing = [test_name for test_name in expected_tests if test_name not in outcomes]
    unexpected = [test_name for test_name, _ in results if test_name not in expected_tests]
    failed = {
        test_name
        for test_name, outcome in results
        if test_name in expected_tests and outcome == "FAILED"
    }
    failed_cases = failed.union(missing)
    if status != 0 and not failed_cases:
        failed_cases.add(expected_tests[0])
    elif status == 0 and unexpected and not failed_cases:
        failed_cases.add(expected_tests[0])

    count = len(expected_tests)
    skipped_cases = {
        test_name
        for test_name in expected_tests
        if outcomes.get(test_name, "").startswith("ignored")
        and test_name not in failed_cases
    }
    root = ET.Element(
        "testsuites",
        name="wasmtime-selected-tests",
        tests=str(count),
        failures=str(len(failed_cases)),
        errors="0",
        skipped=str(len(skipped_cases)),
        time="0",
    )
    suite = ET.SubElement(
        root,
        "testsuite",
        name=name,
        tests=str(count),
        failures=str(len(failed_cases)),
        errors="0",
        skipped=str(len(skipped_cases)),
        time="0",
    )
    for test_name in expected_tests:
        case = ET.SubElement(suite, "testcase", name=test_name, classname=name, time="0")
        outcome = outcomes.get(test_name)
        if test_name in missing:
            message = (
                f"cargo exited with status {status} before this test ran"
                if status
                else "expected test was not discovered"
            )
            ET.SubElement(case, "failure", message=message)
        elif test_name in failed_cases:
            message = "test failed" if outcome == "FAILED" else f"cargo exited with status {status}"
            ET.SubElement(case, "failure", message=message)
        elif test_name in skipped_cases:
            ET.SubElement(case, "skipped")
    ET.SubElement(suite, "system-out").text = log[-200_000:]
    ET.ElementTree(root).write(report_path, encoding="utf-8", xml_declaration=True)
    sys.exit(3 if status == 0 and (failed_cases or unexpected) else 0)

if not results:
    root = ET.Element(
        "testsuites",
        name="wasmtime-selected-tests",
        tests="1",
        failures="1",
        errors="0",
        time="0",
    )
    suite = ET.SubElement(
        root,
        "testsuite",
        name=name,
        tests="1",
        failures="1",
        errors="0",
        skipped="0",
        time="0",
    )
    case = ET.SubElement(suite, "testcase", name=name, classname="cargo", time="0")
    message = f"cargo exited with status {status}" if status else "no tests were discovered"
    failure = ET.SubElement(case, "failure", message=message)
    failure.text = log[-200_000:]
    ET.ElementTree(root).write(report_path, encoding="utf-8", xml_declaration=True)
    sys.exit(3 if status == 0 else 0)

parsed_failures = sum(outcome == "FAILED" for _, outcome in results)
command_failure = status != 0 and parsed_failures == 0
failures = parsed_failures + int(command_failure)
skipped = sum(outcome.startswith("ignored") for _, outcome in results)
test_count = len(results) + int(command_failure)
root = ET.Element(
    "testsuites",
    name="wasmtime-selected-tests",
    tests=str(test_count),
    failures=str(failures),
    errors="0",
    skipped=str(skipped),
    time="0",
)
suite = ET.SubElement(
    root,
    "testsuite",
    name=name,
    tests=str(test_count),
    failures=str(failures),
    errors="0",
    skipped=str(skipped),
    time="0",
)
for test_name, outcome in results:
    case = ET.SubElement(suite, "testcase", name=test_name, classname=name, time="0")
    if outcome == "FAILED":
        failure = ET.SubElement(case, "failure", message="test failed")
        failure.text = log[-200_000:]
    elif outcome.startswith("ignored"):
        ET.SubElement(case, "skipped")
if command_failure:
    case = ET.SubElement(suite, "testcase", name=f"{name}-command", classname="cargo", time="0")
    failure = ET.SubElement(case, "failure", message=f"cargo exited with status {status}")
    failure.text = log[-200_000:]
ET.SubElement(suite, "system-out").text = log[-200_000:]
ET.ElementTree(root).write(report_path, encoding="utf-8", xml_declaration=True)
PY
}

note_status() {
  local status="$1"
  if [ "$status" -ne 0 ] && [ "$STATUS" -eq 0 ]; then
    STATUS="$status"
  fi
}

run_rust_tests() {
  local name="$1"
  local report="$RUN_DIR/$name.xml"
  local log="$RUN_DIR/$name.log"
  local expected_tests=()
  if [ "$name" = "rust-page-size-1-pool" ]; then
    expected_tests=("${PAGE_SIZE_1_RUST_TESTS[@]}")
  fi
  shift

  cargo test \
    --locked \
    --no-fail-fast \
    "$@" 2>&1 | tee "$log"
  local status=${PIPESTATUS[0]}

  write_cargo_report "$name" "$status" "$log" "$report" "${expected_tests[@]}"
  local report_status=$?
  if [ "$status" -eq 0 ] && [ "$report_status" -ne 0 ]; then
    status=1
  fi
  REPORTS+=("$report")
  note_status "$status"
}

configure_c_api_headers() {
  cmake -S crates/c-api -B "$C_API_HEADER_DIR" \
    -DBUILD_TESTS=OFF \
    -DWASMTIME_ALWAYS_BUILD=OFF \
    -DWASMTIME_DISABLE_ALL_FEATURES=ON \
    -DWASMTIME_FEATURE_ASYNC=ON \
    -DWASMTIME_FEATURE_PROFILING=ON \
    -DWASMTIME_FEATURE_CACHE=ON \
    -DWASMTIME_FEATURE_THREADS=ON \
    -DWASMTIME_FEATURE_GC=ON \
    -DWASMTIME_FEATURE_CRANELIFT=ON \
    -DWASMTIME_FEATURE_WAT=ON \
    -DWASMTIME_FEATURE_POOLING_ALLOCATOR=ON \
    -DWASMTIME_FEATURE_COMPONENT_MODEL=ON
}

build_c_api() {
  cargo build \
    --locked \
    -p wasmtime-c-api \
    --no-default-features \
    --features "$C_API_FEATURES"
}

compile_cpp() {
  local source="$1"
  local output="$2"
  shift 2
  c++ -std=c++20 \
    -I crates/c-api/include \
    -I "$C_API_HEADER_DIR/include" \
    "$source" \
    -L "$C_API_LIBRARY_DIR" \
    "-Wl,-rpath,$PWD/$C_API_LIBRARY_DIR" \
    -lwasmtime \
    "$@" \
    -pthread -ldl -lm \
    -o "$output"
}

compile_c() {
  local source="$1"
  local output="$2"

  cc -std=c11 \
    -I crates/c-api/include \
    -I "$C_API_HEADER_DIR/include" \
    "$source" \
    -L "$C_API_LIBRARY_DIR" \
    "-Wl,-rpath,$PWD/$C_API_LIBRARY_DIR" \
    -lwasmtime \
    -pthread -ldl -lm \
    -o "$output"
}

run_c_api_regression() {
  local report="$RUN_DIR/c-api-regression.xml"
  local log="$RUN_DIR/c-api-regression.log"
  local binary="$RUN_DIR/c-api-config-test"

  (
    build_c_api &&
      configure_c_api_headers &&
      compile_cpp crates/c-api/tests/config.cc "$binary" -lgtest_main -lgtest &&
      "$binary" \
        --gtest_color=no \
        --gtest_filter=PoolAllocationConfig.Smoke \
        "--gtest_output=xml:$report"
  ) 2>&1 | tee "$log"
  local status=${PIPESTATUS[0]}

  if [ ! -f "$report" ]; then
    write_command_report "c-api-regression" "$status" "$log" "$report"
  fi
  REPORTS+=("$report")
  note_status "$status"
}

run_c_api_new() {
  local report="$RUN_DIR/c-api-page-size-1-pool.xml"
  local log="$RUN_DIR/c-api-page-size-1-pool.log"
  local cpp_binary="$RUN_DIR/c-api-page-size-1-pool-cpp"

  (
    build_c_api &&
      configure_c_api_headers &&
      compile_cpp crates/c-api/tests/page_size_1_pool_config_b76df6.cc "$cpp_binary" &&
      "$cpp_binary"
  ) 2>&1 | tee "$log"
  local status=${PIPESTATUS[0]}

  write_command_report "c-api-page-size-1-pool" "$status" "$log" "$report"
  REPORTS+=("$report")
  note_status "$status"
}

run_c_api_direct_new() {
  local report="$RUN_DIR/c-api-page-size-1-pool-direct.xml"
  local log="$RUN_DIR/c-api-page-size-1-pool-direct.log"
  local binary="$RUN_DIR/c-api-page-size-1-pool-c"

  (
    build_c_api &&
      configure_c_api_headers &&
      compile_c crates/c-api/tests/page_size_1_pool_config_b76df6.c "$binary" &&
      "$binary"
  ) 2>&1 | tee "$log"
  local status=${PIPESTATUS[0]}

  write_command_report "c-api-page-size-1-pool-direct" "$status" "$log" "$report"
  REPORTS+=("$report")
  note_status "$status"
}

merge_reports() {
  python3 - "$OUTPUT_PATH" "${REPORTS[@]}" <<'PY'
import pathlib
import sys
import xml.etree.ElementTree as ET

output = pathlib.Path(sys.argv[1])
reports = [pathlib.Path(path) for path in sys.argv[2:]]
root = ET.Element("testsuites", name="wasmtime-selected-tests")
tests = failures = errors = skipped = 0
elapsed = 0.0

for report in reports:
    parsed = ET.parse(report).getroot()
    suites = list(parsed) if parsed.tag == "testsuites" else [parsed]
    for suite in suites:
        root.append(suite)
        tests += int(suite.get("tests", "0"))
        failures += int(suite.get("failures", "0"))
        errors += int(suite.get("errors", "0"))
        skipped += int(suite.get("skipped", suite.get("disabled", "0")))
        elapsed += float(suite.get("time", "0") or 0)

root.set("tests", str(tests))
root.set("failures", str(failures))
root.set("errors", str(errors))
root.set("skipped", str(skipped))
root.set("time", f"{elapsed:.6f}")
output.parent.mkdir(parents=True, exist_ok=True)
ET.ElementTree(root).write(output, encoding="utf-8", xml_declaration=True)
PY
}

case "$MODE" in
  base)
    run_rust_tests rust-pooling-regression \
      --manifest-path /app/Cargo.toml \
      --package wasmtime-cli \
      --test all \
      --no-default-features \
      --features "$RUST_FEATURES" \
      -- \
      pooling_allocator:: \
      --test-threads=4
    run_c_api_regression
    ;;
  new)
    run_rust_tests rust-page-size-1-pool \
      --manifest-path /app/Cargo.toml \
      --package wasmtime-cli \
      --test page_size_1_pool_b76df6 \
      --no-default-features \
      --features "$RUST_FEATURES" \
      -- \
      --test-threads=4
    run_c_api_new
    run_c_api_direct_new
    ;;
  *)
    echo "unknown mode: $MODE (expected base or new)" >&2
    exit 2
    ;;
esac

merge_reports || STATUS=1
exit "$STATUS"

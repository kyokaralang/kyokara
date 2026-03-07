# Performance Harness

This document is the canonical process for Kyokara's local performance harness.

## Commands

Record the current machine's baseline:

```sh
cargo run -p xtask -- perf record
```

Check the current machine against its committed baseline:

```sh
cargo run -p xtask -- perf check
```

During development, limit the run to one benchmark case:

```sh
cargo run -p xtask -- perf record --case wordfreq_map_set_run
cargo run -p xtask -- perf check --case wordfreq_map_set_run
```

Both commands:
- build `kyokara-cli` in `--release` once up front
- run the release binary directly
- print a human-readable summary table
- write a machine-readable report to `target/perf/latest.json`

## Corpus Layout

Benchmark cases live under:

```text
tools/perf/cases/<case_id>/
```

Each case directory contains:
- `bench.json`
- the Kyokara source files for that case

`bench.json` schema:

```json
{
  "id": "wordfreq_map_set_run",
  "mode": "run",
  "entry": "main.ky",
  "project": false,
  "expected_stdout": "123\n",
  "expected_ok": true,
  "warmup_runs": 1,
  "measured_runs": 5,
  "max_regression_pct": 20.0,
  "max_regression_ms": 15.0
}
```

Rules:
- `mode` is exactly `run` or `check`
- `id` must match the case directory name
- `entry` is relative to the case directory
- `project` controls whether `--project` is passed to `kyokara`
- `run` cases must set `expected_stdout` and must not set `expected_ok`
- `check` cases must set `expected_ok: true` and must not set `expected_stdout`
- case discovery order is lexical by directory name

Representative v1 cases:
- `bitset_dense_relation_run` intentionally stresses dense `(before, after)` precedence lookups backed by `MutableBitSet`, so regressions in the packed-bit path are visible without falling back to `MutableList<Bool>` proxies.
- `mutable_bool_dense_relation_run` keeps the same dense precedence workload on the legacy `MutableList<Bool>` representation, so the packed-bit win is measurable on a like-for-like algorithm.
- `cow_collection_chain_run` intentionally stresses immutable same-name rebinding on `List`, `Map`, `Set`, and `Deque` so COW-path regressions are visible.

## Baselines

Committed baselines live under:

```text
tools/perf/baselines/
```

Each baseline file is fingerprint-specific and uses this schema:

```json
{
  "schema_version": 1,
  "fingerprint": {
    "os": "macos",
    "arch": "aarch64",
    "cpu_model": "Apple ...",
    "rustc": "rustc 1.xx.x",
    "profile": "release-lto-fat"
  },
  "results": [
    {
      "id": "wordfreq_map_set_run",
      "mode": "run",
      "samples_ms": [12.1, 11.8, 12.0, 11.9, 12.2],
      "median_ms": 12.0
    }
  ]
}
```

## Fingerprint-Gated Behavior

The harness matches baselines by exact fingerprint:
- `os`
- `arch`
- `cpu_model`
- `rustc`
- release profile name

Behavior:
- no matching baseline: `perf check` fails with a clear message telling you to run `perf record`
- multiple matching baselines: `perf check` fails and refuses to guess
- exact match: medians are compared case-by-case

`--case <id>` is a partial workflow:
- `perf record --case <id>` refreshes that one case in the matching baseline file
- `perf check --case <id>` compares only that case against the matching baseline

## Threshold Rule

The harness compares the current median against the baseline median for each case.

A case is a regression only when both are true:
- `current_ms - baseline_ms > max_regression_ms`
- `((current_ms / baseline_ms) - 1.0) * 100.0 > max_regression_pct`

Consequences:
- small noisy deltas do not fail
- large slowdowns fail
- improvements never fail

When checking the full corpus, missing or extra case IDs are treated as a hard failure until the baseline is refreshed.

## Latest Report

The latest run is always written to:

```text
target/perf/latest.json
```

This report includes:
- command (`record` or `check`)
- machine fingerprint
- selected case results
- baseline medians when applicable
- pass/regression status per case

## Refresh Workflow

Refresh the committed baseline after an intentional performance change:

1. Make the performance change.
2. Rebuild and verify correctness.
3. Run:

```sh
cargo run -p xtask -- perf record
```

4. Review the updated baseline JSON under `tools/perf/baselines/`.
5. Re-run:

```sh
cargo run -p xtask -- perf check
```

6. Commit the code change and the baseline update together.

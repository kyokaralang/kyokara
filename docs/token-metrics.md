# Token Metrics Workflow

This document defines the repeatable token-count workflow used for test harness
size reduction work.

## Tool

- Script: `tools/repo_tokens.py`
- Encoding default: `cl100k_base`
- Input set: tracked files from Git (`git ls-files` or `git ls-tree` with `--rev`)

Install dependency:

```sh
python3 -m pip install --user tiktoken
```

## Standard Commands

### 1) Full tracked repo snapshot (current worktree)

```sh
python3 tools/repo_tokens.py
```

### 2) Rust-only snapshot

```sh
python3 tools/repo_tokens.py --include '**/*.rs'
```

### 3) Snapshot for a specific revision

```sh
python3 tools/repo_tokens.py --rev origin/main
```

### 4) Top heavy files

```sh
python3 tools/repo_tokens.py --top 20
```

### 5) Machine-readable output

```sh
python3 tools/repo_tokens.py --json > /tmp/repo_tokens.json
```

## Baseline Reference (2026-02-28)

From `main` before test-harness cleanup:

- Total tracked lines: `53,858`
- Total tracked tokens (`cl100k_base`): `421,671`
- Rust-only tokens (`*.rs`, `cl100k_base`): `359,959`

## Notes

- Files that are not UTF-8 decodable are skipped and reported.
- For reproducible comparisons, use `--rev` to avoid local worktree differences.

# Test Harness Token Report (2026-02-28)

Scope: `codex/test-harness-tidy` compared to `origin/main`.

## Commands Used

```sh
# full tracked-files token counts
python3 tools/repo_tokens.py --json > /tmp/kyokara_tokens_head.json
python3 tools/repo_tokens.py --rev origin/main --json > /tmp/kyokara_tokens_base.json

# rust-only token counts
python3 tools/repo_tokens.py --json --include '**/*.rs'
python3 tools/repo_tokens.py --rev origin/main --json --include '**/*.rs'

# per-crate test counts
cargo test -q -p kyokara-api -- --list | rg ': test$' | wc -l
cargo test -q -p kyokara-eval -- --list | rg ': test$' | wc -l
cargo test -q -p kyokara-codegen -- --list | rg ': test$' | wc -l
cargo test -q -p kyokara-refactor -- --list | rg ': test$' | wc -l
```

## Repository-Level Token Delta

| metric | origin/main | branch | delta |
|---|---:|---:|---:|
| full tracked-files tokens (`cl100k_base`) | 425,479 | 422,771 | -2,708 |
| Rust-only tokens (`**/*.rs`) | 363,389 | 360,524 | -2,865 |

## Touched Test Files Delta

| file | tokens (base) | tokens (branch) | delta | lines (base) | lines (branch) | delta |
|---|---:|---:|---:|---:|---:|---:|
| `crates/api/tests/api_tests.rs` | 29,931 | 29,085 | -846 | 3,755 | 3,625 | -130 |
| `crates/eval/tests/eval_tests.rs` | 33,202 | 32,901 | -301 | 4,241 | 4,216 | -25 |
| `crates/codegen/tests/codegen_tests.rs` | 12,937 | 12,381 | -556 | 1,795 | 1,694 | -101 |
| `crates/refactor/tests/refactor_tests.rs` | 11,388 | 10,226 | -1,162 | 1,360 | 1,231 | -129 |
| **total** | **87,458** | **84,593** | **-2,865** | **11,151** | **10,766** | **-385** |

## Test Count Parity (or Justified Delta)

| crate | origin/main | branch | delta |
|---|---:|---:|---:|
| `kyokara-api` | 164 | 148 | -16 |
| `kyokara-eval` | 374 | 374 | 0 |
| `kyokara-codegen` | 150 | 125 | -25 |
| `kyokara-refactor` | 36 | 36 | 0 |

Justification for reduced test counts:

- API and Codegen suites intentionally consolidated many near-identical tests into
  table-driven tests with per-case assertions.
- This reduces boilerplate while preserving behavioral checks per input case.
- Eval and Refactor kept test-count parity while still reducing setup duplication.

## Quality Gate

Validated on branch:

- `cargo fmt --all`
- `cargo test -q`
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`

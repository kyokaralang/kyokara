# AI Programmer Special Features Tracker

This is a living tracker for AI-beneficial language/tooling features.
Each row must stay linked to at least one GitHub issue.

## Completeness Rubric

| Score | Meaning |
|---|---|
| `0%` | Not started; only problem statement exists. |
| `25%` | Design/intent captured in issues, little to no implementation. |
| `50%` | Partial implementation exists but core gaps remain. |
| `75%` | Functionally present, but hardening/docs/coverage still incomplete. |
| `100%` | Implemented, tested, and docs aligned with behavior. |

## A) Wishlist Features (AI-native)

| ID | Special feature (why AI benefits) | Assessment | Completeness | GitHub issue(s) |
|---|---|---|---|---|
| `W1` | Machine-readable diagnostics + fix patches (agents can apply fixes without prose parsing) | Implemented | `100%` | [#36](https://github.com/kyokaralang/kyokara/issues/36), [#37](https://github.com/kyokaralang/kyokara/issues/37) |
| `W2` | Stable symbol IDs (robust cross-edit references) | Implemented | `100%` | [#41](https://github.com/kyokaralang/kyokara/issues/41) |
| `W3` | Round-trippable/lossless syntax tree (safe machine rewrites) | Implemented | `100%` | [#8](https://github.com/kyokaralang/kyokara/issues/8) |
| `W4` | Effect system with compile-time checks (side effects are statically visible) | Implemented (core) | `80%` | [#15](https://github.com/kyokaralang/kyokara/issues/15) |
| `W5` | Deterministic/reproducible builds (agent outputs are stable across environments) | Missing | `10%` | [#233](https://github.com/kyokaralang/kyokara/issues/233) |
| `W6` | No implicit behavior policy (avoid ambiguous globals/ambient behavior) | Partial | `60%` | [#236](https://github.com/kyokaralang/kyokara/issues/236) |
| `W7` | Rich machine-readable API metadata (effects/fallibility/deprecation/examples) | Partial | `45%` | [#237](https://github.com/kyokaralang/kyokara/issues/237) |
| `W8` | Refactoring protocol with verified safe patches | Partial | `70%` | [#31](https://github.com/kyokaralang/kyokara/issues/31), [#32](https://github.com/kyokaralang/kyokara/issues/32), [#238](https://github.com/kyokaralang/kyokara/issues/238) |
| `W9` | Capability-native runtime permissions (deny-by-default execution boundary) | Implemented (with known limits) | `85%` | [#22](https://github.com/kyokaralang/kyokara/issues/22), [#186](https://github.com/kyokaralang/kyokara/issues/186) |
| `W10` | Executable docs in CI (prevent stale guidance for agents) | Missing | `10%` | [#234](https://github.com/kyokaralang/kyokara/issues/234) |
| `W11` | Deterministic canonical formatter (predictable edits) | Implemented | `95%` | [#34](https://github.com/kyokaralang/kyokara/issues/34) |
| `W12` | Typed serialization contracts + schema evolution policy | Missing | `5%` | [#235](https://github.com/kyokaralang/kyokara/issues/235) |
| `W13` | Partial-program checking (typed holes + incomplete code support) | Implemented | `100%` | [#35](https://github.com/kyokaralang/kyokara/issues/35) |
| `W14` | Fast incremental compiler query API (low-latency AI edit loops) | Partial/weak | `35%` | [#33](https://github.com/kyokaralang/kyokara/issues/33), [#239](https://github.com/kyokaralang/kyokara/issues/239) |
| `W15` | Constrained metaprogramming policy (typed/hygienic/inspectable if added) | Deferred | `20%` | [#242](https://github.com/kyokaralang/kyokara/issues/242) |

## B) Documented AI-Beneficial Features Not In The Wishlist List

These are in existing language/design docs but were not explicitly in the 15-item wishlist.

| ID | Additional documented feature | Assessment | Completeness | GitHub issue(s) |
|---|---|---|---|---|
| `D1` | Contracts as first-class syntax (`requires`/`ensures`/`old`) | Implemented + ongoing verification work | `80%` | [#9](https://github.com/kyokaralang/kyokara/issues/9), [#28](https://github.com/kyokaralang/kyokara/issues/28), [#30](https://github.com/kyokaralang/kyokara/issues/30) |
| `D2` | Property-based testing integrated in language workflow | Implemented + expansion ongoing | `85%` | [#23](https://github.com/kyokaralang/kyokara/issues/23), [#200](https://github.com/kyokaralang/kyokara/issues/200), [#25](https://github.com/kyokaralang/kyokara/issues/25) |
| `D3` | Deterministic replay logging/execution for auditability | In progress | `40%` | [#26](https://github.com/kyokaralang/kyokara/issues/26), [#27](https://github.com/kyokaralang/kyokara/issues/27) |
| `D4` | Refactor transactions with verify-before-apply behavior | Implemented | `90%` | [#32](https://github.com/kyokaralang/kyokara/issues/32), [#190](https://github.com/kyokaralang/kyokara/issues/190), [#191](https://github.com/kyokaralang/kyokara/issues/191) |
| `D5` | Compiler-as-API outputs for AI loops (diagnostics/symbol graph/holes) | Implemented with contract drift risk | `75%` | [#36](https://github.com/kyokaralang/kyokara/issues/36), [#38](https://github.com/kyokaralang/kyokara/issues/38), [#241](https://github.com/kyokaralang/kyokara/issues/241) |
| `D6` | LSP support for interactive coding loops | Implemented baseline, stronger incrementality pending | `70%` | [#33](https://github.com/kyokaralang/kyokara/issues/33), [#239](https://github.com/kyokaralang/kyokara/issues/239) |
| `D7` | API surface law (canonical placement/order/pipe compatibility for AI generation) | RFC drafted, rollout pending | `35%` | [#243](https://github.com/kyokaralang/kyokara/issues/243), [#236](https://github.com/kyokaralang/kyokara/issues/236), [#238](https://github.com/kyokaralang/kyokara/issues/238) |

## C) Active Docs-vs-Implementation Drift

| ID | Drift item | Assessment | Completeness | GitHub issue(s) |
|---|---|---|---|---|
| `X1` | Replay CLI is documented as available in multiple docs, but runtime/CLI path is not fully exposed | Open drift | `30%` | [#240](https://github.com/kyokaralang/kyokara/issues/240), [#26](https://github.com/kyokaralang/kyokara/issues/26), [#27](https://github.com/kyokaralang/kyokara/issues/27) |
| `X2` | `typed_ast.json` is documented, but current `kyokara-api` check output omits it | Open drift | `20%` | [#241](https://github.com/kyokaralang/kyokara/issues/241), [#39](https://github.com/kyokaralang/kyokara/issues/39) |

## Update Protocol

1. When an issue state changes, update the matching row's `Assessment` and `Completeness`.
2. If work lands without an issue link, create one immediately and add it to the row.
3. If docs and implementation diverge, add/update a drift row in section C.
4. Keep this file in sync with `README.md`, `docs/design-v0.md`, `llms.txt`, and `llms-full.txt`.

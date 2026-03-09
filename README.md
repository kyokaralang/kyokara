# Kyokara

**A programming language designed for machines to write, verify, and refactor — and for humans to trust.**

AI-assisted coding is becoming dominant in many workflows. Kyokara assumes the primary author may be an agent. Every design decision — from the type system to the runtime — optimizes for machine generation, machine verification, and machine refactoring, while producing code that humans can audit at a glance.

**North star:** Given a diff, a reviewer can identify all side effects from types alone. Compiler refactors never break typechecking. Most fixes are machine-applicable — no natural-language parsing needed.

## Why Kyokara?

The languages AI writes *in* were never designed for this:

- **Generated code is hard to trust.** You can't tell what it does to your filesystem, your network, your database — until it's too late.
- **Compilers talk to humans, not machines.** Error messages are prose. Fixes require interpretation. AI wastes tokens guessing what went wrong.
- **Refactoring is fragile.** AI does regex-level find-and-replace on code that deserves semantic transformations.
- **"It worked once" isn't good enough.** There's no way to replay what happened, or systematically verify behavior.

## The Six Ideas

### 1. Explicit Capabilities

Every side effect is declared and sandboxed. A function that touches the network says so in its type. The runtime enforces it with a deny-by-default capability manifest. No surprises.

```kyokara
fn fetch_rate(base: Currency, quote: Currency) -> Result<Float, HttpError>
with net
{
  Http.get(url: "https://rates.example/api") |> parse_rate(base, quote)
}
```

A pure function has no `with` clause. It *cannot* do I/O. This isn't a convention — it's a compiler guarantee.

Current runtime scope:
- Manifest checks are enforced for capability presence (built-in `io`/`fs` intrinsics + user `with <effect>` functions). Built-in capability names are canonical, case-sensitive lowercase (`io`, `fs`).
- Fine-grained manifest fields (`allow_domains`, `allow_tables`, `allow_keys`) are fail-closed right now: if any are present, execution is rejected until resource-aware host operations are implemented.

### 2. Deterministic Replay

Every effectful execution produces a replay log. The runtime executes effects through a single handler interface that records each request and response. Run the same log back and get identical behavior.

```sh
kyokara run job.ky --caps manifest.json    # produces run.log
kyokara replay run.log                     # exact reproduction
kyokara replay run.log --mode=verify       # compare against live
```

**Scope:** Determinism is guaranteed for the language runtime plus recorded effects under single-threaded execution, with captured inputs (time, network, database responses). Concurrency scheduling and external state outside the recorded boundary are not covered.

### 3. Contracts as Code

Preconditions, postconditions, and property tests are first-class language constructs — not comments, not docstrings, not a separate test framework. They're checked at runtime by default. Static verification via SMT is opt-in and scoped to a decidable fragment (see roadmap).

```kyokara
fn withdraw(acct: Account, amt: Money) -> Result<Account, WithdrawError>
contract
  requires (amt.amount > 0)
  requires (amt.currency == acct.balance.currency)
  ensures (match (result) {
    Ok(a) => a.balance.amount == old(acct.balance.amount) - amt.amount
    Err(_) => true
  })
{
  ...
}
```

Legacy direct-clause form (without `contract`) is invalid in v0.

```kyokara
property sort_idempotent(xs: List<Int> <- Gen.auto()) {
  xs.sort().sort() == xs.sort()
}
```

**Verification staging:**
1. Contracts are runtime assertions + property tests (QuickCheck-style). *(v0.1)*
2. SMT for a restricted fragment: linear arithmetic + uninterpreted functions, no heap. Best-effort, never blocks compilation. *(v0.3+)*

### 4. Typed Holes and Partial Compilation

Code doesn't have to be finished to be useful. Kyokara compiles incomplete programs — holes carry their expected type, available variables, purity constraints, and contract obligations. The compiler tells the AI exactly what's needed to fill each gap.

```kyokara
fn normalize_email(s: String) -> String {
  let trimmed = s.trim()
  let lowered = _   // hole: expects String, must be pure
  lowered
}
```

### 5. Compiler as API

The compiler doesn't print error messages — it emits structured JSON. Default `check --format json` output contains diagnostics, hole specs, and symbol graph; typed AST is available via explicit opt-in (`--emit typed-ast`). AI agents consume compiler output directly, no prose parsing required.

```json
{
  "diagnostics": [{"severity": "error", "span": {...}, "fix": {"patch": "..."}}],
  "holes": [{"id": 0, "expected_type": "String", "effects": [], "inputs": ["trimmed: String"]}],
  "symbol_graph": {
    "functions": [{"id": "fn::add", "name": "add", "calls": ["fn::helper"], ...}],
    "types": [{"id": "type::Color", "variants": [{"id": "type::Color::Red", ...}], ...}],
    "capabilities": [{"id": "cap::io", "functions": ["cap::io::read"], ...}]
  }
}
```

Typed AST opt-in:

```sh
kyokara check file.ky --format json --emit typed-ast
```

### 6. Semantic Refactoring

Refactors are compiler operations, not text transformations. Rename a symbol across the codebase — functions, types, capabilities, variants — in single-file or multi-file projects. Add missing match cases or capability annotations from existing diagnostics. Each refactor is wrapped in a transaction: edits are applied in-memory, the type checker re-runs, and the result reports whether the refactored code is still valid. The CLI only writes to disk when verification passes (or when `--force` is set).

```sh
# Rename a function across files
kyokara refactor main.ky --action rename --symbol add --new-name sum

# Apply edits directly to disk
kyokara refactor main.ky --action rename --symbol add --new-name sum --apply

# Add missing match arms at a specific offset
kyokara refactor file.ky --action add-missing-match-cases --offset 42

# In multi-file projects, --target-file disambiguates which module the offset refers to
kyokara refactor main.ky --action add-missing-capability --offset 42 --target-file math.ky

# Project mode auto-detects for main.ky; use --project to force it for other entry files
kyokara refactor other.ky --action rename --symbol foo --new-name bar --project
```

## Language at a Glance

```kyokara
// math.ky
pub type Currency = USD | IDR | EUR

type Money = { amount: Int, currency: Currency }

pub fn add_fee(x: Money, fee_bps: Int) -> Money {
  let fee = x.amount * fee_bps / 10_000
  Money { amount: x.amount + fee, currency: x.currency }
}

pub fn currency_symbol(c: Currency) -> String {
  match (c) {
    USD => "$",
    IDR => "Rp",
    EUR => "€",
  }
}
```

```kyokara
// main.ky
import math

fn main() -> Int {
  let result = add_fee(Money { amount: 1000, currency: USD }, 250)
  result.amount
}
```

Pure by default. Private by default — use `pub` to export. No null — use `Option<T>`. No exceptions — use `Result<T, E>`. Exhaustive pattern matching enforced by the compiler. Pipeline operator (`|>`) and error propagation (`?`) for clean data flow. Convention-based modules — file path determines module path, `import` brings public names into scope. Local bindings are immutable by default with `let`; use `var` for loop-local mutable state and `x = expr` to reassign an existing mutable local. In v1, lambdas may not capture mutable locals. Collection constructors live under `collections.*`: `collections.List.new()`, `collections.Map.new()`, `collections.Set.new()`, `collections.BitSet.new(size)`, plus specialized `collections.MutableList.new()`, `collections.MutableList.from_list(xs)`, `collections.MutableMap.new()`, `collections.MutableSet.new()`, `collections.MutableBitSet.new(size)`, `collections.Deque.new()`, and `collections.MutablePriorityQueue.new_min()/new_max()`. `List` includes immutable index updates (`set`/`update`) and direct traversal (`map/filter/fold/...`). `collections.Deque` provides persistent queue-style operations (`push_front`/`push_back`/`pop_front`/`pop_back`) plus the same traversal surface. `collections.MutableList` provides alias-visible in-place updates for dense-index and stack-style workloads (`push`/`last`/`pop`/`extend`/`set`/`update`). `collections.MutableMap` and `collections.MutableSet` provide alias-visible mutable key/value and set storage for `Hash + Eq` keys/elements; for nominal keys like points, derive or implement those traits explicitly, e.g. `type Point derive(Eq, Hash) = { x: Int, y: Int }`. `MutableMap` also exposes `get_or_insert_with(key, fn() => value)` as the canonical compute-if-absent helper for memoization and cache-style code. `collections.MutablePriorityQueue` provides alias-visible prioritized worklists with explicit min/max construction, stable insertion-order ties for equal priorities, and `push/peek/pop/len/is_empty`. `BitSet`/`MutableBitSet` provide packed dense bounded-bit storage with `test/set/reset/flip`, set algebra, and ascending `values()` traversal. Traversal constructors are surface-level expressions: `start..<end` for half-open integer ranges and `seed.unfold(step)` for stateful generation. Canonical traversal style is collection-first (`xs.map(...).filter(...).count()` for pipeline reuse, `xs.count(fn(...) => ...)` for direct predicate counts, `xs.frequencies()` for histogram/tally queries). For predicate/search traversal, default to `any`/`all`/`find`/`count(predicate)`; use `frequencies()` when you need the whole bucket-count map; reserve `fold` for true accumulation/reduction. Loop control syntax is statement-only: `while (cond) { ... }`, `for (pattern in source) { ... }`, `break`, `continue`; `for` patterns support destructuring but must be irrefutable. Pure no-owner digest helpers live under `hash.*`, with `import hash` and `hash.md5(text)` as the canonical MD5 entry point; `String.md5()` remains available as convenience sugar over the same lowercase hexadecimal digest. `String` surface is Unicode-scalar based: `len`, indexing, `substring`, and `chars()` operate on Unicode scalars. `Char` exposes `to_string()`, `code()`, `is_decimal_digit()`, `to_decimal_digit()`, and `to_digit(radix)`; digit helpers use ASCII digit/letter semantics (`0-9`, `a-z`, `A-Z`), and `to_digit(radix)` requires `radix` in `2..=36`. `Int` includes `pow` as the canonical integer exponentiation method and native bitwise operators `&`, `|`, `^`, `~`, `<<`, `>>` (`>>` is arithmetic; shifts require counts in `0..63`).

```kyokara
fn loop_examples(xs: List<Int>, s: String) -> Int {
  var acc = 0

  for (n in 0..<10) { if ((n % 2) == 1) { acc = acc + n } }             // range
  for (x in xs) { if (x > 0) { acc = acc + 1 } }                        // collection
  for (line in s.lines()) { if (line.len() > 0) { acc = acc + 1 } }     // producer

  acc
}
```

Bitwise operators are native on `Int`, so small machine simulators and binary-state code map directly:

```kyokara
fn step(state: Int) -> Int {
  let mixed = state ^ (state << 6)
  let masked = mixed & ~(1 << 1)
  masked | (mixed >> 5)
}
```

## Architecture

Kyokara's compiler follows [rust-analyzer](https://github.com/rust-analyzer/rust-analyzer)'s proven architecture:

```
Source → Lexer (logos) → Parser (tree-agnostic) → CST (rowan, lossless)
     → HIR (def/ty/facade) → Type Inference → Effect Checking
     → KyokaraIR (SSA, block-params) → Codegen (WASM)
     → API (structured JSON output)
```

**Glossary:** **CST** = Concrete Syntax Tree (lossless, preserves whitespace/comments). **HIR** = High-level Intermediate Representation (desugared, typed, used for analysis). **KIR** (KyokaraIR) = SSA-based IR with block parameters, reuses HIR types directly. Instructions include Const, Binary, Unary, record/ADT ops, Call (direct/indirect/intrinsic), Assert, Hole, FnRef, BlockParam, FnParam, AdtFieldGet. The validator enforces type invariants (Bool conditions, Fn call targets, Record/Adt field access bases), structural invariants (predecessor edges, duplicate cases), and reference validity. The lowering pass handles ADT switch with dedup/catch-all/nested-literal checks, sequential match with early termination, contract `old()` pre-state, and function references.

The parser emits events, not trees — so it can be tested without a CST library. The HIR is split into data definitions, type checking, and a query facade — so the interpreter can use the data without the checker. The API crate owns all serialization — so internal types stay clean.

```
crates/
  stdx          # shared utilities
  span          # source locations
  intern        # string interning
  diagnostics   # error types
  parser        # tree-agnostic recursive-descent parser
  syntax        # lossless CST (rowan + logos) + typed AST wrappers
  hir-def       # HIR data types + CST→HIR lowering + name resolution
  hir-ty        # type inference + effect checking
  hir           # semantic query facade
  kir           # SSA-based IR (block params, text format, validator, HIR→KIR lowering)
  codegen       # WASM code generation (KIR → wasm-encoder)
  eval          # tree-walking interpreter
  fmt           # canonical code formatter (Wadler-Lindig Doc IR)
  refactor      # semantic refactor engine (rename, quickfix)
  lsp           # LSP server with salsa incrementality
  api           # JSON serialization of all compiler outputs
  cli           # kyokara binary
```

## Performance Harness

Kyokara ships a local, fingerprint-gated performance harness for regression tracking:

```sh
cargo run -p xtask -- perf record
cargo run -p xtask -- perf check
```

For focused iteration on one benchmark:

```sh
cargo run -p xtask -- perf record --case wordfreq_map_set_run
cargo run -p xtask -- perf check --case wordfreq_map_set_run
```

The canonical process, corpus schema, and baseline policy live in [`docs/performance-harness.md`](docs/performance-harness.md).

## Roadmap

| Version | What ships | Status |
|---------|-----------|--------|
| **v0.0** | Parser ✓, name resolution ✓, CST→HIR lowering ✓, type checker ✓, effect checking ✓, typed holes ✓, structured diagnostics ✓, hole specs ✓, symbol graph ✓, patch suggestions ✓ | **Complete** |
| **v0.1** | Tree-walking interpreter ✓, intrinsics ✓, builtin Option/Result types ✓, canonical formatter ✓, stable symbol IDs ✓, runtime contracts ✓, core stdlib (List/MutableList/Deque traversal, MutableMap/MutableSet/MutablePriorityQueue, Map, Set, BitSet/MutableBitSet, String, Int/Float) ✓ | **Complete** |
| **v0.2** | Module system (convention-based layout, `pub` visibility, flat imports) ✓, refactor engine (rename, add missing match cases, add missing capability) ✓, refactor transactions (atomic verify-before-apply) ✓, capability enforcement (type-level E0011 + runtime manifest `--caps`) ✓, LSP server (diagnostics, hover, go-to-def, references, completion, code actions, formatting) ✓ | **Complete** |
| **v0.3** | KyokaraIR data structures ✓, HIR→KIR lowering ✓, WASM codegen MVP (scalars, control flow, calls, ADTs, records) ✓, property-based testing (`kyokara test --explore`) ✓, SMT verification (restricted fragment), capability sandbox, deterministic replay | In progress |

AI-special-feature status tracking (with completeness scores + issue links) lives in [docs/design-v0.md#16-ai-first-feature-tracker](docs/design-v0.md#16-ai-first-feature-tracker).
API surface design rules live in [docs/rfcs/0001-api-surface-law.md](docs/rfcs/0001-api-surface-law.md), with traversal-surface specifics in [docs/rfcs/0002-collection-first-traversal-surface.md](docs/rfcs/0002-collection-first-traversal-surface.md), constructor-surface specifics in [docs/rfcs/0003-opaque-traversal-constructor-surface.md](docs/rfcs/0003-opaque-traversal-constructor-surface.md), module/capability placement model in [docs/rfcs/0004-module-taxonomy-and-capability-boundaries.md](docs/rfcs/0004-module-taxonomy-and-capability-boundaries.md), mutable collection naming/placement policy in [docs/rfcs/0005-mutable-collection-naming-and-placement.md](docs/rfcs/0005-mutable-collection-naming-and-placement.md), loop-control surface policy in [docs/rfcs/0006-loop-control-surface.md](docs/rfcs/0006-loop-control-surface.md), compiler API typed-AST opt-in contract in [docs/rfcs/0007-optional-typed-ast-api-output.md](docs/rfcs/0007-optional-typed-ast-api-output.md), dense-bit collection policy in [docs/rfcs/0010-bitset-surface.md](docs/rfcs/0010-bitset-surface.md), and local mutable binding policy in [docs/rfcs/0013-local-var-bindings.md](docs/rfcs/0013-local-var-bindings.md).
Core dispatch shadow-safety and temporary constructor reservation are documented in [docs/design-v0.md#10-standard-library-v0-minimum](docs/design-v0.md#10-standard-library-v0-minimum) (qualified-constructor follow-up: [#293](https://github.com/kyokaralang/kyokara/issues/293)).
Canonical documentation map lives in [docs/INDEX.md](docs/INDEX.md).

## FAQ

**Why build the compiler frontend before an interpreter? Most new languages start with "make something run."**

Kyokara's primary user is an AI agent, not a human at a REPL. The agent's workflow is: write code, ask the compiler what's wrong, read structured JSON, apply fixes, repeat. That feedback loop doesn't need execution — it needs analysis. So we built the part that *thinks about* programs first (type checking, effect checking, exhaustiveness, hole specs, symbol graph, fix suggestions). The interpreter arrived in v0.1 as a straightforward tree-walker over the HIR, after the analysis pipeline was already in place.

**Can I run Kyokara programs right now?**

Yes. You can write `.ky` files and: `kyokara check file.ky --format json` (type-check), `kyokara run file.ky` (interpret), or `kyokara fmt file.ky` (format). Check gives you structured diagnostics, typed hole specs, symbol graph, and machine-applicable fix patches; `--emit typed-ast` adds typed AST when needed. Run executes via a tree-walking interpreter. Fmt enforces canonical formatting.

**Why Rust?**

WASM target alignment (the compiler can eventually compile to WASM itself), performance for compiler tooling, strong ecosystem for parsers and WASM runtimes, and memory safety without GC overhead.

**Why not use salsa/incremental computation from the start?**

Full recompute per invocation is fine for v0.0. Salsa gets added when the LSP (v0.2+) needs incrementality. Premature incrementality would complicate the codebase without a user who benefits from it yet.

**How is this different from just using TypeScript/Rust/etc. with an AI?**

Those languages weren't designed for machine authorship. Their error messages are prose that AI has to interpret. Their type systems don't track side effects. Their compilers don't emit structured fix patches. Kyokara makes the compiler-to-agent interface a first-class design concern — not an afterthought.

## Building

```sh
# Requires Rust (stable)
cargo build

# Type-check a file
cargo run -p kyokara-cli -- check <file.ky>

# JSON output (for AI agents)
cargo run -p kyokara-cli -- check <file.ky> --format json

# Run a single file
cargo run -p kyokara-cli -- run <file.ky>

# Run a multi-file project (auto-detected when sibling .ky files exist)
cargo run -p kyokara-cli -- run examples/modules/main.ky

# Run the traversal usage catalog example
cargo run -p kyokara-cli -- run examples/seq.ky

# Format a file (writes back)
cargo run -p kyokara-cli -- fmt <file.ky>

# Check formatting without writing (exits 1 if not formatted)
cargo run -p kyokara-cli -- fmt --check <file.ky>

# Refactor: rename a symbol (prints JSON edits)
cargo run -p kyokara-cli -- refactor <file.ky> --action rename --symbol add --new-name sum

# Refactor: apply edits to disk (only if verification passes)
cargo run -p kyokara-cli -- refactor <file.ky> --action rename --symbol add --new-name sum --apply

# Refactor: apply edits even if verification fails
cargo run -p kyokara-cli -- refactor <file.ky> --action rename --symbol add --new-name sum --apply --force

# Refactor: rename a type or variant
cargo run -p kyokara-cli -- refactor <file.ky> --action rename --symbol Color --new-name Hue --kind type

# Property-based test: explore contract functions with random inputs
cargo run -p kyokara-cli -- test <file.ky> --explore

# PBT with fixed seed and 200 tests per function
cargo run -p kyokara-cli -- test <file.ky> --explore --num-tests 200 --seed 42

# PBT: replay saved corpus only (CI-safe, no random generation)
cargo run -p kyokara-cli -- test <file.ky>

# PBT: JSON output
cargo run -p kyokara-cli -- test <file.ky> --explore --format json
```

## Token Metrics

Kyokara includes a repo-token utility for AI-context budgeting:

```sh
# Full tracked repo tokens (current worktree)
python3 tools/repo_tokens.py

# Rust-only tokens (all tracked .rs files)
python3 tools/repo_tokens.py --include '**/*.rs'

# Compare against main without checkout
python3 tools/repo_tokens.py --rev origin/main

# Show top 20 token-heavy files
python3 tools/repo_tokens.py --top 20
```

The tool uses `tiktoken` with `cl100k_base` by default.  
If needed: `python3 -m pip install --user tiktoken`

See [`docs/token-metrics.md`](docs/token-metrics.md) for workflow details.
Latest cleanup report: [`docs/reports/2026-02-28-test-harness-token-report.md`](docs/reports/2026-02-28-test-harness-token-report.md).

## License

[MIT](LICENSE)

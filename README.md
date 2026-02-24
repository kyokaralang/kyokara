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
fn fetch_rate(base: Currency, quote: Currency) -> Result[Float, HttpError] with Net =
  Http.get(url = "https://rates.example/api") |> parse_rate(base, quote)
```

A pure function has no `with` clause. It *cannot* do I/O. This isn't a convention — it's a compiler guarantee.

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
fn withdraw(acct: Account, amt: Money) -> Result[Account, WithdrawError]
  requires amt.amount > 0
  requires amt.currency == acct.balance.currency
  ensures  match result:
             | Ok(a) -> a.balance.amount == old(acct.balance.amount) - amt.amount
             | Err(_) -> true
=
  ...
```

```kyokara
property "sort is idempotent" for all xs: List[Int] =
  List.sort(List.sort(xs)) == List.sort(xs)
```

**Verification staging:**
1. Contracts are runtime assertions + property tests (QuickCheck-style). *(v0.1)*
2. SMT for a restricted fragment: linear arithmetic + uninterpreted functions, no heap. Best-effort, never blocks compilation. *(v0.3+)*

### 4. Typed Holes and Partial Compilation

Code doesn't have to be finished to be useful. Kyokara compiles incomplete programs — holes carry their expected type, available variables, purity constraints, and contract obligations. The compiler tells the AI exactly what's needed to fill each gap.

```kyokara
fn normalize_email(s: String) -> String =
  let trimmed = String.trim(s)
  let lowered = ?lowercase(trimmed)   // hole: expects String -> String, must be pure
  lowered
```

### 5. Compiler as API

The compiler doesn't print error messages — it emits structured JSON. Diagnostics, typed ASTs, symbol graphs, hole specifications, and machine-applicable fix patches. AI agents consume compiler output directly, no parsing required.

```json
{
  "diagnostics": [{"severity": "error", "span": {...}, "fix": {"patch": "..."}}],
  "holes": [{"id": 0, "expected_type": "String", "effects": [], "inputs": ["trimmed: String"]}],
  "symbol_graph": {"nodes": [...], "edges": [...]}
}
```

### 6. Semantic Refactoring

Refactors are compiler operations, not text transformations. Rename a symbol across the codebase. Extract a function. Inline a call. Each refactor is a transaction that returns a patch *and* a verification status — did the types still check? Did the contracts still hold?

## Language at a Glance

```kyokara
module finance.payments

import std.list as List

type Currency = | USD | IDR | EUR

type Money = { amount: Int, currency: Currency }

fn add_fee(x: Money, fee_bps: Int) -> Money =
  let fee = x.amount * fee_bps / 10_000
  { amount = x.amount + fee, currency = x.currency }

fn currency_symbol(c: Currency) -> String =
  match c:
    | USD -> "$"
    | IDR -> "Rp"
    | EUR -> "€"
```

Pure by default. No null — use `Option[T]`. No exceptions — use `Result[T, E]`. Exhaustive pattern matching enforced by the compiler. Pipeline operator (`|>`) and error propagation (`?`) for clean data flow.

## Architecture

Kyokara's compiler follows [rust-analyzer](https://github.com/rust-analyzer/rust-analyzer)'s proven architecture:

```
Source → Lexer (logos) → Parser (tree-agnostic) → CST (rowan, lossless)
     → HIR (def/ty/facade) → Type Inference → Effect Checking
     → KyokaraIR (SSA, planned) → Codegen (WASM, planned)
     → API (structured JSON output)
```

**Glossary:** **CST** = Concrete Syntax Tree (lossless, preserves whitespace/comments). **HIR** = High-level Intermediate Representation (desugared, typed, used for analysis). **IR** (KyokaraIR) = low-level Intermediate Representation (SSA-based, used for optimization and codegen — planned for v0.3).

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
  api           # JSON serialization of all compiler outputs
  cli           # kyokara binary
```

## Roadmap

| Version | What ships | Status |
|---------|-----------|--------|
| **v0.0** | Parser, name resolution, CST→HIR lowering, type checker, effect checking, typed holes, structured diagnostics | In progress |
| **v0.1** | Canonical formatter, stable symbol IDs, desugaring, tree-walking interpreter, runtime contracts, core stdlib | Planned |
| **v0.2** | Refactor engine, LSP server, capability enforcement, module/package system | Planned |
| **v0.3** | Property testing, SMT verification (restricted fragment), WASM codegen, capability sandbox, deterministic replay | Planned |

## Building

```sh
# Requires Rust (stable)
cargo build

# Run the compiler
cargo run -p kyokara-cli -- check <file.ky>
```

## License

[MIT](LICENSE)

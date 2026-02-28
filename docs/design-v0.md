# Kyokara Language + Runtime (AI-First) — Implementation-Oriented Spec (v0)

This document summarizes the agreed design direction for **Kyokara**: an AI-first programming language and platform optimized for machine generation, machine verification, and machine refactoring. Humans are reviewers/operators; the compiler and runtime are designed to support agentic coding loops.

> Primary wedge: **capability-sandboxed automation/workflow functions** with **deterministic replay** and **auditability**.

---

## 0. Product framing

Kyokara is not "a better general-purpose language" first. It is a **permissioned, deterministic automation runtime** with a language designed to make AI-produced code:
- easier to generate correctly,
- easier to verify incrementally,
- easier to refactor mechanically,
- safer to run in production by construction.

Core differentiators:
1) **Explicit effects/capabilities**
2) **Deterministic replay**
3) **Contracts + property tests as first-class**
4) **Typed holes + partial compilation**
5) **Compiler as an API (structured diagnostics + patch suggestions)**
6) **Semantic refactor engine (transactional refactors)**

---

## 1. Design goals (AI-first)

### 1.1 Validity gradient
Support "almost valid" programs:
- parse partial ASTs,
- typecheck as far as possible,
- preserve holes and incomplete nodes,
- emit structured constraints so an AI can complete missing parts.

### 1.2 Intent is explicit and checkable
First-class specification:
- preconditions/postconditions (`requires`, `ensures`, `invariant`)
- property-based tests (`property ... for all ...`)
- optional refinement constraints (`type PositiveInt = Int where x>0`)

### 1.3 Determinism by default
- pure by default
- effects must be explicit
- deterministic execution mode with replay logs

### 1.4 Refactoring is a first-order operation
- canonical formatting
- unambiguous syntax
- stable symbol graph
- semantic edits via compiler APIs (not regex diffs)

---

## 2. Surface language (v0)

### 2.1 Syntax principles
- keyword-led constructs
- minimal ambiguous precedence
- prefer explicit forms over clever syntax
- support named arguments by default

### 2.2 Modules and imports

**Implemented (v0.2):** Convention-based file layout — the file path determines the module path. No explicit `module` declarations needed.

```
project/
  main.ky          → root module (entry point)
  math.ky          → module "math"
  math/utils.ky    → module "math.utils"
```

Visibility is **private by default**. Use the `pub` keyword to export items:

```kyokara
// math.ky
pub fn add(x: Int, y: Int) -> Int { x + y }
pub fn double(x: Int) -> Int { x * 2 }
fn internal_helper(x: Int) -> Int { x + 1 }  // not visible to importers
```

Import a module to bring its public names into scope:

```kyokara
// main.ky
import math

fn main() -> Int {
    let x = add(10, 20)   // pub fn from math.ky
    let y = double(x)     // pub fn from math.ky
    y
}
```

Rules:
* `import math` brings all `pub` items from `math.ky` into the importing module's scope as flat names (no `math.add()` qualified paths yet).
* Private items (without `pub`) are not visible across module boundaries.
* Local definitions shadow imports.
* Importing a module does **not** grant its capability. Capabilities are separate from libraries.

**Planned:** Qualified access (`math.add()`), aliased imports (`import math as M`), selective imports (`import math { add, double }`).

### 2.3 Types

#### Records

```kyokara
type Money = { amount: Int, currency: Currency }
```

#### ADTs (tagged unions)

```kyokara
type Currency =
  | USD
  | IDR
  | EUR

type Result[T, E] =
  | Ok(value: T)
  | Err(error: E)

type Option[T] =
  | Some(value: T)
  | None
```

Rules:
* no `null` in the language; use `Option[T]`.

### 2.4 Functions

Purity default:

```kyokara
fn add_fee(x: Money, fee_bps: Int) -> Money =
  let fee = { amount = x.amount * fee_bps / 10_000, currency = x.currency }
  { amount = x.amount + fee.amount, currency = x.currency }
```

### 2.5 Capabilities / effects

Declare capabilities:

```kyokara
cap Net
cap Clock
cap Db
cap Secrets
```

Annotate effect requirements:

```kyokara
fn fetch_rate(base: Currency, quote: Currency) -> Result[Float, HttpError] with Net =
  Http.get(url = "...") |> parse_rate(base=base, quote=quote)
```

Rules:
* a function without `with ...` is pure and cannot invoke effectful operations.
* callers must "inherit" required caps unless the cap is introduced explicitly via scoped blocks (optional v0 feature).

Open design questions (to resolve before hir-ty):
* **Effect polymorphism**: higher-order functions need effect-polymorphic signatures, e.g. `fn map(f: fn(A) -> B with e, xs: List[A]) -> List[B] with e`. Without this, the stdlib will be painful.
* **Subeffecting**: is `Pure` a subeffect of every capability set? Can `with Net` call a `Pure` function? (Yes — effects are an upper bound, "may do", not "must do".)
* **Scoped capabilities**: can a caller restrict a capability before passing it? e.g. `with caps.restrict(domain="rates.example")`.
* **Async**: if concurrency is added later, effect tracking must compose with async. Deferring concurrency to post-v0 avoids this for now.

### 2.6 Pattern matching

```kyokara
fn currency_symbol(c: Currency) -> String =
  match c:
    | USD -> "$"
    | IDR -> "Rp"
    | EUR -> "€"
```

Compiler enforces exhaustiveness.

### 2.7 Contracts

```kyokara
fn withdraw(acct: Account, amt: Money) -> Result[Account, WithdrawError]
  requires amt.amount > 0
  requires amt.currency == acct.balance.currency
  ensures  match result:
             | Ok(a2) -> a2.balance.amount == old(acct.balance.amount) - amt.amount
             | Err(_) -> true
=
  ...
```

`old(expr)` refers to pre-state.

### 2.8 Property-based tests

```kyokara
property sort_idempotent(xs: List[Int]) {
  List.sort(List.sort(xs)) == List.sort(xs)
}

property add_commutative(a: Int, b: Int) {
  a + b == b + a
}
```

Property bodies are lowered and type-checked (must return `Bool`). The PBT runner discovers properties alongside contracted functions and tests them with generated inputs.

### 2.9 Typed holes + partial compilation

Holes are legal syntax:

```kyokara
fn normalize_email(s: String) -> String =
  let trimmed = String.trim(s)
  let lowered = ?lowercase(trimmed)   // expression hole
  lowered
```

Rules:
* holes carry an expected type from context.
* compiler emits machine-readable hole specs (constraints, expected type, purity/effect constraints, input vars).
* programs with holes may compile to "partial artifacts" (typed AST + partial IR), but are not runnable unless holes resolved.

---

## 3. Desugaring rules (make semantics explicit)

### 3.1 Pipeline operator

Surface:

```kyokara
x |> f(a=1, b=2)
```

Desugars to:

```kyokara
f(x, a=1, b=2)
```

Rules:
* pipeline feeds the first parameter positionally.
* optionally, function declarations can mark an explicit pipe parameter:

  ```kyokara
  fn split(text: String, sep: String) -> List[String] pipe text = ...
  ```

  then `|>` binds to that parameter.

### 3.2 Error propagation sugar

Provide `?` postfix sugar to propagate `Err`:

```kyokara
let body = Http.get(url=... )?
```

Desugars to a `match` returning early on `Err`.

### 3.3 Contracts lowering

* `requires`: pre-state checks
* `ensures`: post-state obligations referencing saved `old(...)` values

Verification policy:
* v0.1: contracts are runtime assertions + property tests (QuickCheck-style)
* v0.3: `--verify` flag attempts best-effort SMT proofs for a **restricted fragment** (linear arithmetic + uninterpreted functions, no heap reasoning). Reports what it could/couldn't discharge.
* compilation is **never** blocked on verification results
* "verified" means "SMT discharged proof obligation" — sound within the modeled fragment, incomplete by design

Capturable expressions in `old(...)`:
* function parameters and any pure expression in scope at function entry

---

## 4. Type system (v0 stance)

### 4.1 Strong static typing

* local inference allowed
* explicit types required at module/public API boundaries (recommended)
* avoid implicit coercions; explicit casts only

### 4.2 ADTs + pattern matching

* exhaustive matching required unless `_` wildcard is present

### 4.3 Refinements (optional v0 / v0.5)

Support:

```kyokara
type PositiveInt = Int where x > 0
```

Rule:
* statically discharge when provable, else insert runtime checks.

---

## 5. Concurrency (defer heavy design)

v0 default:
* structured async (`async/await`) if needed
* avoid nondeterministic primitives by default
* provide deterministic test mode if concurrency included at all

Given wedge (automation), start with:
* single-threaded deterministic execution
* explicit concurrency later

---

## 6. Core Intermediate Representation (KyokaraIR) — minimal typed core

KyokaraIR is the low-level **Intermediate Representation (IR)** — a compiler-internal form of the program that sits between the HIR (High-level IR, used for type checking and analysis) and the final codegen target (WASM). It is fully typed, SSA-based, and designed for optimization and verification.

**Implementation status:** The `kir` crate implements the data structures, text format printer, well-formedness validator, builder API, and HIR→KIR lowering pass. Key design choices: block parameters instead of phi nodes (Cranelift/MLIR style), reuses `hir_ty::Ty` directly (no parallel type system), arena-based storage via `la_arena`. The lowering pass walks the typed HIR expression tree and emits flat SSA instructions with explicit control-flow blocks — if/else becomes Branch+merge, match becomes Switch (ADT) or chained Branch (sequential), contracts become Assert instructions with `old()` pre-state preservation, function references become FnRef values.

**WASM codegen (MVP):** The `codegen` crate compiles KIR to WASM binary via `wasm-encoder`. Value representation: Int→i64, Float→f64, Bool/Unit→i32, ADT/Record→i32 (pointer into linear memory). Linear memory with bump allocator (1 page initial, no deallocation). ADTs use `[tag:i32][pad:4][field0:8]...` layout with uniform 8-byte slots. Records use sorted-by-name fields without a tag header. Control flow: structured emission with nested if/else/end for branches, block/br_table for switch. Deferred: closures, strings, lists, maps, intrinsics, capabilities.

**Validator invariants:** The well-formedness validator enforces: value reference validity, block parameter count/type matching against branch arguments, terminator presence, entry block has zero parameters, return type consistency, Bool type for Branch/Assert conditions, Record/Adt base type for field access, Adt base type for ADT field extraction, Fn type for indirect call targets, no duplicate Switch case variants, and no block parameters without predecessor edges (excluding unreachable blocks).

**Lowering details:** The HIR→KIR lowering pass reads param types and return type from `InferenceResult` (not from expression types). ADT match lowering deduplicates constructor cases, stops after catch-all arms, and falls back to sequential lowering when arms contain unsupported patterns. Sequential match lowering marks the merge block unreachable when all arms terminate early and stops dispatch after wildcard/bind arms. Nested literal subpatterns inside ADT constructors emit equality checks with branch-to-fallback on mismatch.

### 6.1 Requirements

KyokaraIR must be:
* fully typed
* effect-annotated per function
* explicit control flow (SSA recommended)
* carry hole nodes (typed + constraints)
* suitable for optimization and verification hooks

### 6.2 IR primitives

* constants, vars
* record construct/access/update
* ADT construct + field extraction
* `switch` on ADT tags with binder unpacking
* calls (direct, indirect, intrinsic)
* function references (first-class `FnRef`)
* explicit error-return paths
* `assert` nodes (contracts)

### 6.3 Effects

Each function has an effect set:
* `effects {}` for pure
* `effects {Net, Db, Clock, Secrets}` etc.

Calls require the caller's effect set to be a superset.

### 6.4 Holes in IR

Represented as:

```
hole#id : Type
constraints:
  - must be pure (or allowed effects)
  - must satisfy contract obligations X
inputs: <typed vars>
```

---

## 7. Toolchain (language + platform)

Kyokara's toolchain is a core part of the spec.

### 7.1 Compilation pipeline

1. Parser -> lossless CST (preserve formatting/comments)
2. Elaborator -> typed AST (names/types/effects)
3. Verifier -> proof obligations from contracts/properties
4. Lowerer -> KyokaraIR (SSA)
5. Optimizer -> optimized IR
6. Codegen -> pick one target for v0 (WASM recommended for sandboxing)
7. Runtime -> capability sandbox + deterministic replay + structured errors
8. LSP/Refactor engine -> semantic edits and transactions

### 7.2 Compiler as API (non-negotiable)

Compiler emits:
* `diagnostics.json`: structured errors, spans, expected vs actual types/effects, suggested patches
* `typed_ast.json` or binary form
* `symbol_graph.json`: call graph, type graph, effect graph — each node carries a stable namespaced ID (`fn::name`, `type::Name`, `type::Name::Variant`, `cap::Name`, `cap::Name::method`)
* `hole_specs.json`: each hole's expected type and constraints
* `patches.json`: machine-applicable edits for common fixes

### 7.3 Refactor transactions

Provide refactors with guarantees:
* rename symbol
* extract/inline function
* move module
* add missing match cases
* add missing capability annotations
* convert positional args to named args

Each returns:
* patch
* status: `typechecked`, `verified_level_1`, `tests_updated`, etc.

---

## 8. Runtime (wedge-defining features)

### 8.1 Capability sandbox (least privilege) ✓

Runtime enforces capabilities via a JSON manifest loaded with `--caps`:
* `kyokara run program.ky --caps caps.json`
* deny-by-default: when a manifest is present, only listed capabilities are allowed
* no manifest = allow-all (backward compatible)
* optional fine-grained allowlists (domains, tables, secret names) parsed but not yet enforced

Enforcement points:
* **Intrinsic I/O** — `print` and `println` require the `"IO"` capability
* **User-defined capabilities** — functions with `with Console` etc. are checked against the manifest at call time
* **Pure functions** — never denied regardless of manifest

Example manifest:

```json
{
  "caps": {
    "Net": { "allow_domains": ["rates.example", "api.partner.com"] },
    "Db":  { "allow_tables": ["payments", "users"] },
    "Secrets": { "allow_keys": ["PAYMENTS_API_KEY"] },
    "Clock": {},
    "IO": {}
  }
}
```

Implementation: `CapabilityManifest` and `CapabilityGrant` types in `kyokara-eval::manifest`, loaded via `run_with_manifest()` and `run_project_with_manifest()`. The `Interpreter` checks capabilities before dispatching intrinsics and before entering user functions with `with_caps` annotations.

### 8.2 Deterministic replay

Runtime logs all effectful interactions through a single effect handler interface:
* network requests/responses
* time reads
* randomness
* db reads/writes (results + commit decisions)

Execution modes:
* `kyokara run job.ky --caps caps.json` -> produces `run.log`
* `kyokara replay run.log` -> reproduces behavior deterministically

Replay policy:
* replay mode is **read-only by default** — all write effects become no-ops that return the logged result.
* `--replay-mode=verify` compares what *would* have been written against the log and reports mismatches.

Determinism boundary:
* determinism guarantee holds for **the language runtime + recorded effects** under **single-threaded execution**, with **captured inputs** (time, network responses, database results).
* anything outside the recorded boundary (external state changes, concurrent processes) is not covered.
* concurrency scheduling replay is deferred — v0 is single-threaded by design.

### 8.3 Sandboxed execution target

Recommended v0: compile to WASM with host functions implementing capabilities.

WASM limitations to address:
* GC: use linear memory + arena allocation or WASM GC proposal
* I/O: all I/O goes through host function capability layer
* Threads: single-threaded for v0 (aligns with determinism goal)

### 8.4 Capability violation errors ✓

Capability violations at runtime produce a structured `CapabilityDenied` error (not a panic). This error includes:
* the capability requested (e.g. `"IO"`, `"Console"`)
* the function that requested it (e.g. `"Println"` for intrinsics, `"greet"` for user functions)

Error format: `capability denied: {capability} (required by \`{function}\`)`

---

## 9. Error model

### 9.1 Result-based errors

`Result[T, E]` is the sole error channel. There are no exceptions.

### 9.2 Panic

Panics exist only for unrecoverable programmer errors (e.g., contract violations in strict mode, integer overflow in debug mode). Panics are not catchable.

### 9.3 Error propagation across capabilities

Effectful functions return `Result`. Capability violations produce `CapabilityDenied` errors that propagate through the normal `Result` channel.

---

## 10. Standard library (v0 minimum)

v0 stdlib is implemented as intrinsic functions in the eval crate.
Builtin types `Option<T>`, `Result<T, E>`, `List<T>`, and `Map<K, V>` are
injected as synthetic types before type-checking.

**Implemented (v0.1):**
* `Option<T>` — builtin ADT (`Some(T) | None`), used as return type for safe lookups ✓
* `Result<T, E>` — builtin ADT (`Ok(T) | Err(E)`), `?` propagation works ✓
* `List<T>` — opaque builtin type backed by `Vec<Value>` ✓
  * `list_new`, `list_push`, `list_len`, `list_get` → `Option<T>`, `list_head` → `Option<T>`, `list_tail`, `list_is_empty`, `list_reverse`, `list_concat`
  * Higher-order: `list_map`, `list_filter`, `list_fold`
* `Map<K, V>` — opaque builtin type backed by `Vec<(Value, Value)>` (insertion-order) ✓
  * `map_new`, `map_insert`, `map_get` → `Option<V>`, `map_contains`, `map_remove`, `map_len`, `map_keys` → `List<K>`, `map_values` → `List<V>`, `map_is_empty`
* String operations ✓ — `string_len` (char count), `string_contains`, `string_starts_with`, `string_ends_with`, `string_trim`, `string_split` → `List<String>`, `string_substring`, `string_to_upper`, `string_to_lower`, `char_to_string`
* Int/Float math ✓ — `abs`, `min`, `max`, `float_abs`, `float_min`, `float_max`, `int_to_float`, `float_to_int`
* I/O ✓ — `print`, `println`, `int_to_string`, `string_concat`

**Planned (v0.2+):**
* `std.io` — richer text I/O (requires `IO` cap)
* `std.test` — property test generators and assertions

---

## 11. v0 scope (what to ship first)

### 11.1 Implementation milestones

**v0.0 — AI-Facing Compiler**
* Parser with error recovery (lossless CST) ✓
* Typed AST wrappers over CST (AstNode trait, typed accessors) ✓
* Name resolution with scope chains (local → type params → module → constructors → imports) ✓
* CST→HIR lowering: item tree collection (Pass 1) + body lowering (Pass 2) ✓
* Pipeline `|>` and propagation `?` desugaring at HIR level ✓
* Duplicate definition and unresolved name diagnostics ✓
* Type checker (ADTs, generics, pattern matching exhaustiveness) ✓
* Effect/capability checking ✓
* Typed holes + partial compilation ✓
* Structured diagnostics (`diagnostics.json` with error codes, spans, expected/actual types) ✓
* Typed hole specs (`hole_specs.json` with expected type, available variables, effect constraints) ✓
* Symbol graph (function/type/capability nodes, call edges, effect annotations) ✓
* Patch suggestions (machine-applicable fixes for E0009 MissingMatchArms, E0011 EffectViolation) ✓

**v0.1 — Tooling Foundation + Interpreter**
* Tree-walking interpreter (`kyokara run <file>`, `kyokara-eval` crate) ✓
* Intrinsic functions (print, println, int_to_string, string_concat) ✓
* Builtin `Option<T>` and `Result<T, E>` types (injected as synthetic ADTs; `?` works out of the box) ✓
* Canonical formatter (`kyokara fmt`, `kyokara-fmt` crate, Wadler-Lindig Doc IR) ✓
* Stable symbol IDs (`kind::name` / `kind::parent::child` format, unique across symbol kinds) ✓
* Runtime contract checks (requires/ensures/old) ✓
* Core stdlib (List, Map, String, Int/Float — 43 intrinsic functions total) ✓

**v0.2 — Refactoring + LSP + Capabilities**
* Module system: convention-based file layout, `pub` visibility, flat imports ✓
* Refactor engine: rename symbol (single-file + multi-file), add missing match cases, add missing capability annotation ✓ — CST-based, post-refactor verification, structured TextEdit patches
* Refactor transactions: atomic refactor operations with in-memory re-check ✓ — `transact()` / `transact_project()` apply edits, re-run the type checker, and return `VerificationStatus` (Verified / Failed / Skipped). CLI gates `--apply` on verification passing; `--force` bypasses. API returns `"typechecked"` / `"failed"` / `"skipped"` status with structured verification diagnostics (message, code, span). Quickfix actions accept `--target-file` to disambiguate which module an offset refers to in project mode. CLI auto-detects project mode for `main.ky` with sibling `.ky` files; `--project` flag forces project mode for other entry files.
* LSP server: salsa incrementality, diagnostics, hover, go-to-definition, find references, completion, code actions (quickfixes), formatting ✓
* Capability enforcement: type-level checking (E0011) ✓ + runtime manifest enforcement (`--caps`, deny-by-default) ✓

**v0.3 — Verification + Codegen + Replay**
* Property-based test harness ✓ (`pbt` crate: choice-sequence engine, type-driven generators, 4-pass shrinker, corpus persistence; `kyokara test <file> --explore` discovers contract functions and explicit `property` declarations, generates random inputs, checks contracts/properties, shrinks counterexamples)
* SMT integration for contract verification (restricted fragment: linear arithmetic + uninterpreted functions, best-effort, never blocks compilation)
* KyokaraIR data structures ✓ (SSA, block params, text format, validator) + HIR→KIR lowering ✓ + WASM codegen MVP ✓ (scalars, control flow, function calls, ADTs, records via `codegen` crate + `wasm-encoder`; deferred: closures, strings, lists, maps, intrinsics, capabilities)
* Capability sandbox runtime (host functions + manifest)
* Deterministic replay logging and execution (single-threaded, recorded effects)

### 11.2 Cut from v0 (defer)

* macros/metaprogramming
* dependent types / heavy theorem proving
* complex ownership model
* advanced concurrency
* large stdlib (keep it small and canonical)

---

## 12. Repository structure

One monorepo for early velocity. Rust workspace with fine-grained crates:

```
kyokara/
  crates/
    stdx/          # shared utilities (leaf, no kyokara deps)
    span/          # FileId, Span, TextRange
    intern/        # string interning (lasso)
    diagnostics/   # Diagnostic, Severity, Fix
    parser/        # tree-agnostic recursive-descent parser (SyntaxKind + Events)
    syntax/        # lossless CST (rowan + logos) + typed AST wrappers
    hir-def/       # HIR data types, CST→HIR lowering, name resolution
    hir-ty/        # type inference, exhaustiveness, effect checking
    hir/           # semantic query facade
    eval/          # tree-walking interpreter
    fmt/           # canonical code formatter (Wadler-Lindig Doc IR)
    refactor/      # semantic refactor engine (rename, quickfix)
    lsp/           # LSP server with salsa incrementality
    pbt/           # property-based testing (generators, shrinker, corpus)
    api/           # compiler-as-API, JSON serialization DTOs
    cli/           # kyokara binary (check / run / fmt / refactor / test / lsp)
  docs/            # design docs
  spec/            # formal grammar
```

Crate dependency DAG follows rust-analyzer's layered pattern: parser is tree-agnostic (no rowan), HIR is split into def/ty/facade, API crate owns all serde.

---

## 13. Naming notes

Brand: **Kyokara**
Product: **Kyokara Runtime**
Core concept mapping:
* explicit caps -> `Net`, `Db`, `Secrets`, `Clock`
* evidence/audit -> contracts + logs + replay
* AI-first -> holes + structured diagnostics + refactor transactions

---

## 14. Implementation language

Rust is recommended for:
* WASM target alignment (compiler can also compile to WASM itself)
* performance for compiler tooling
* strong ecosystem for parsers (logos, chumsky, tree-sitter), WASM runtimes (wasmtime, wasmer)
* memory safety without GC overhead

---

## 15. Immediate next steps

1. ~~Define exact grammar (PEG/LL(k)) with recoverable parsing.~~ ✓
2. ~~Implement lexer + parser -> lossless CST.~~ ✓
3. ~~Implement typed AST wrappers + CST→HIR lowering + name resolution.~~ ✓
4. ~~Implement type checker (ADTs, generics, exhaustiveness, unification).~~ ✓
5. ~~Implement effect/capability checking.~~ ✓
6. ~~Implement typed holes + partial compilation.~~ ✓
7. ~~Emit structured diagnostics + hole specs.~~ ✓
8. ~~Emit patch suggestions + symbol graph.~~ ✓
9. ~~Implement tree-walking interpreter for rapid iteration.~~ ✓
10. ~~Add contracts as runtime checks.~~ ✓
11. ~~Implement module system (convention-based layout, pub visibility, flat imports).~~ ✓
12. ~~Implement LSP server with salsa incrementality (diagnostics, hover, goto-def, references, completion, code actions, formatting).~~ ✓
13. Implement WASM runtime host functions for capabilities + replay log.
14. ~~Add property test runner and basic generators.~~ ✓
15. Integrate SMT solver for opt-in static verification.

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
- preconditions/postconditions (`contract` section with `requires`/`ensures`/`invariant`)
- property-based tests (`property name(x: T <- Gen.auto()) where (pred) { body }`)
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

API/stdlib surface consistency rules are specified in RFC 0001:
- [API Surface Law (AI-first)](rfcs/0001-api-surface-law.md)
- [Mutable Collection Naming and Placement](rfcs/0005-mutable-collection-naming-and-placement.md)
- [BitSet and MutableBitSet Surface](rfcs/0010-bitset-surface.md)

### 2.2 Modules and imports

**Implemented (v0.2):** Convention-based file layout — the file path determines the module path. There is no source-level `module Path` declaration.

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

Import a module to bind its namespace, or import members directly:

```kyokara
// main.ky
import math
from math import double

fn main() -> Int {
    let x = math.add(10, 20)  // namespace import
    let y = double(x)         // direct member import
    y
}
```

Rules:
* `import math` binds only the namespace `math`, so public members are accessed as `math.add(...)`.
* `import math as m` binds a namespace alias.
* `from math import add, double` binds public members directly into local scope.
* `from math import add as plus` binds a renamed local alias.
* `from Result import Ok, Err` and `from Option import Some, None` bind variants directly from a type path.
* Private items (without `pub`) are not visible across module boundaries.
* Local definitions shadow imports.
* Importing a module does **not** grant its capability. Capabilities are separate from libraries.
* No star imports and no relative imports exist in v1.

### 2.3 Types

#### Records

```kyokara
type Money = { amount: Int, currency: Currency }
```

#### ADTs (tagged unions)

```kyokara
type Currency =
  USD
  | IDR
  | EUR

type Result<T, E> =
  Ok(T)
  | Err(E)

type Option<T> =
  Some(T)
  | None
```

Rules:
* no `null` in the language; use `Option<T>`.

### 2.4 Functions

Purity default:

```kyokara
fn add_fee(x: Money, fee_bps: Int) -> Money {
  let fee = x.amount * fee_bps / 10_000
  Money { amount: x.amount + fee, currency: x.currency }
}
```

### 2.5 Effects

Declare effects:

```kyokara
effect net
effect clock
effect db
effect secrets
```

Annotate effect requirements:

```kyokara
fn fetch_rate(base: Currency, quote: Currency) -> Result<Float, HttpError>
with net
{
  Http.get(url: "...") |> parse_rate(base: base, quote: quote)
}
```

Rules:
* a function without `with ...` is pure and cannot invoke effectful operations.
* callers must "inherit" required effects unless the effect is introduced explicitly via scoped blocks (optional v0 feature).

Open design questions (to resolve before hir-ty):
* **Effect polymorphism**: higher-order functions need effect-polymorphic signatures, e.g. `fn map(f: fn(A) -> B with e, xs: collections.List<A>) -> collections.List<B> with e`. Without this, the stdlib will be painful.
* **Subeffecting**: is `Pure` a subeffect of every capability set? Can `with net` call a `Pure` function? (Yes — effects are an upper bound, "may do", not "must do".)
* **Scoped capabilities**: can a caller restrict a capability before passing it? e.g. `with caps.restrict(domain="rates.example")`.
* **Async**: if concurrency is added later, effect tracking must compose with async. Deferring concurrency to post-v0 avoids this for now.

### 2.6 Pattern matching

```kyokara
fn currency_symbol(c: Currency) -> String {
  match (c) {
    USD => "$",
    IDR => "Rp",
    EUR => "€",
  }
}
```

Compiler enforces exhaustiveness.

### 2.7 Loop control

```kyokara
from collections import List

fn sum_odds(n: Int) -> Int {
  var acc = 0
  for (x in 0..<n) {
    if ((x % 2) == 0) { continue }
    if (x > 1000) { break }
    acc = acc + x
  }
  acc
}

fn count_positive(xs: List<Int>) -> Int {
  var acc = 0
  for (x in xs) {
    if (x > 0) { acc = acc + 1 }
  }
  acc
}

fn non_empty_line_count(s: String) -> Int {
  var acc = 0
  for (line in s.lines()) {
    if (line.len() > 0) { acc = acc + 1 }
  }
  acc
}
```

Rules:
* local bindings are immutable by default with `let`; use `var` for reassignable loop-local state
* statement-only loop/control forms: `while (cond) { ... }`, `for (pattern in source) { ... }`, `break`, `continue`
* assignment uses bare local identifiers only: `x = expr`
* v0.4 restriction: lambdas may not capture mutable locals
* parentheses and braces are mandatory in loop heads/bodies
* `for` source must be traversable (`start..<end`, collections, and producer chains)
* `for` pattern uses full pattern grammar but must be irrefutable (refutable patterns are type errors)

### 2.8 Contracts

```kyokara
fn withdraw(acct: Account, amt: Money) -> Result<Account, WithdrawError>
contract
  requires (amt.amount > 0)
  requires (amt.currency == acct.balance.currency)
  ensures (match (result) {
    Result.Ok(a2) => a2.balance.amount == old(acct.balance.amount) - amt.amount
    Result.Err(_) => true
  })
{
  ...
}
```

Legacy direct-clause form is invalid in v0:

```kyokara
fn withdraw(acct: Account, amt: Money) -> Result<Account, WithdrawError>
  requires (amt.amount > 0)
{
  ...
}
```

`old(expr)` refers to pre-state.

`ensures` expressions evaluate against the function-entry environment plus an implicit `result` binding for the returned value. Direct parameter reads inside `ensures` therefore remain stable even if the body consumed or shadowed those values while producing the result.

### 2.9 Property-based tests

```kyokara
from collections import List

property sort_idempotent(xs: List<Int> <- Gen.auto()) {
  xs.sorted().sorted() == xs.sorted()
}

property add_commutative(a: Int <- Gen.auto(), b: Int <- Gen.auto()) {
  a + b == b + a
}
```

Property parameters use `<-` generator bindings: `name: Type <- Gen.spec()`. Available generators include `Gen.auto()` (type-driven), `Gen.int()`, `Gen.int_range(min, max)`, `Gen.float()`, `Gen.float_range(min, max)`, `Gen.bool()`, `Gen.string()`, `Gen.char()`, `Gen.list(inner)`, `Gen.map(k, v)`, `Gen.option(inner)`, `Gen.result(ok, err)`. Bare parameters (without `<-`) are parse errors.

Property bodies are lowered and type-checked (must return `Bool`). The PBT runner discovers properties alongside contracted functions and tests them with generated inputs.

Optional `where` clauses constrain generated inputs:

```kyokara
property positive_is_positive(x: Int <- Gen.auto())
where (x > 0)
{
  x > 0
}
```

The `where` expression is lowered as a precondition on the property body. Candidates that violate the constraint are discarded (with a budget of 100x the requested test count). The shrinker respects `where` constraints: shrunk counterexamples always satisfy the predicate. Unsatisfiable constraints (all candidates discarded) are reported as errors. Refined types in non-property contexts (regular functions, type aliases) are still rejected.

### 2.10 Typed holes + partial compilation

Holes are legal syntax:

```kyokara
fn normalize_email(s: String) -> String {
  let trimmed = s.trim()
  let lowered = _   // expression hole
  lowered
}
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
x |> f(a: 1, b: 2)
```

Desugars to:

```kyokara
f(x, a: 1, b: 2)
```

Rules:
* pipeline feeds the first parameter positionally.
* call arguments are evaluated left-to-right in source order before parameter-slot binding.
* positional arguments must come before named arguments.

### 3.2 Error propagation sugar

Provide `?` postfix sugar to propagate `Err`:

```kyokara
let body = Http.get(url: "...")?
```

Desugars to a `match` returning early on `Err`.

### 3.3 Integer bitwise operators

Bitwise operators are native on `Int`:

```kyokara
let mixed = state ^ (state << 6)
let masked = mixed & ~(1 << 1)
let next = masked | (mixed >> 5)
```

Rules:
* supported operators: `&`, `|`, `^`, `~`, `<<`, `>>`
* operands are `Int` only
* `>>` is arithmetic right shift (sign-extending)
* shift counts must be in `0..63`; out-of-range counts are errors/traps
* this is the canonical surface for VM-style and binary-state code; do not reimplement XOR or shifts in userland

### 3.4 Contracts lowering

* `requires`: pre-state checks
* `ensures`: post-state obligations evaluated against the function-entry environment plus implicit `result`; `old(...)` captures pre-state expressions explicitly

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

**WASM codegen (current state):** The `codegen` crate compiles KIR to WASM binary via `wasm-encoder`, and `kyokara run --backend wasm` / `kyokara build --target wasm` / `kyokara replay` now use that pipeline for both single-file and project/package-mode programs. Value representation stays Int→i64, Float→f64, Bool/Unit→i32, Char→i32, and heap values (ADT/Record/String/closures/collections)→i32 pointers into linear memory. Linear memory still uses a bump allocator (1 page initial, no deallocation). ADTs use `[tag:i32][pad:4][field0:8]...` layout with uniform 8-byte slots. Records use sorted-by-name fields without a tag header. Current shipped support covers the frozen surface across closures, strings, collections, trait-backed builtins, and the shared replay/capability host ABI.

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
* `effects {net, db, clock, secrets}` etc.

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
* default API output: diagnostics + hole specs + symbol graph
* optional `typed_ast` payload via explicit opt-in (`check_with_options(... include_typed_ast=true)` / `kyokara check --format json --emit typed-ast`)
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
* optional fine-grained allowlists (`allow_domains`, `allow_tables`, `allow_keys`) are parsed and validated fail-closed
* if a manifest contains those fine-grained fields, runtime returns a structured error (`UnsupportedManifestConstraint`) until resource-aware enforcement is implemented

Enforcement points:
* **Intrinsic I/O** — `print` and `println` require the `"io"` capability
* **User-defined capabilities** — functions with `with console` etc. are checked against the manifest at call time
* **Pure functions** — never denied regardless of manifest

Example manifest:

```json
{
  "caps": {
    "net": { "allow_domains": ["rates.example", "api.partner.com"] },
    "db":  { "allow_tables": ["payments", "users"] },
    "secrets": { "allow_keys": ["PAYMENTS_API_KEY"] },
    "clock": {},
    "io": {}
  }
}
```

Implementation: `CapabilityManifest` and `CapabilityGrant` types in `kyokara-eval::manifest`, loaded via `run_with_manifest()` and `run_project_with_manifest()`. Entry points validate unsupported fine-grained constraints before execution, and the `Interpreter` checks capabilities before dispatching intrinsics and before entering user functions with `with_caps` annotations.

### 8.2 Deterministic replay

Runtime logs supported effectful interactions through a single effect handler interface:
* `io.print`
* `io.println`
* `io.read_line`
* `io.read_stdin`
* `fs.read_file`
* capability allow/deny checks for built-in and user-declared effects

Execution modes:
* `kyokara run job.ky --caps caps.json --replay-log run.log` -> writes a replay log
* `kyokara replay run.log` -> reproduces behavior deterministically

Replay policy:
* replay mode is **read-only by default** — all write effects become no-ops that return the logged result.
* `kyokara replay run.log --mode verify` compares what *would* have been written against the log and reports mismatches.

Determinism boundary:
* determinism guarantee holds for **the language runtime + recorded effects** under **single-threaded execution** with source fingerprint validation.
* anything outside the recorded boundary (external state changes, future effect modules, concurrent processes) is not covered.
* concurrency scheduling replay is deferred — v0 is single-threaded by design.

### 8.3 Sandboxed execution target

Recommended v0: compile to WASM with host functions implementing capabilities.

WASM limitations to address:
* GC: use linear memory + arena allocation or WASM GC proposal
* I/O: all I/O goes through host function capability layer
* Threads: single-threaded for v0 (aligns with determinism goal)

### 8.4 Capability violation errors ✓

Capability violations at runtime produce a structured `CapabilityDenied` error (not a panic). This error includes:
* the capability requested (for example `"io"` for built-ins, plus user-defined effect names)
* the function that requested it (e.g. `"Println"` for intrinsics, `"greet"` for user functions)

Error format: `capability denied: {capability} (required by \`{function}\`)`

---

## 9. Error model

### 9.1 Result-based errors

`Result<T, E>` is the sole error channel. There are no exceptions.

### 9.2 Panic

Panics exist only for unrecoverable programmer errors (e.g., contract violations in strict mode, integer overflow in debug mode). Panics are not catchable.

### 9.3 Error propagation across capabilities

Effectful functions return `Result`. Capability violations produce `CapabilityDenied` errors that propagate through the normal `Result` channel.

---

## 10. Standard library (v0 minimum)

v0 stdlib is implemented via intrinsic functions in the eval crate, exposed through
a canonical API surface: method calls for value-owned behavior, module-qualified calls
for no-owner utilities and effects, and type-namespaced constructors.

Canonical visibility matrix:
* Prelude builtin value types (no import): primitives plus `Option<T>`, `Result<T, E>`, `ParseError`, and internal `Seq<T>` support.
* Collection family names (explicit visibility only): `List<T>`, `MutableList<T>`, `MutableMap<K, V>`, `MutableSet<T>`, `MutablePriorityQueue<P, T>`, `BitSet`, `MutableBitSet`, `Deque<T>`, `MutableDeque<T>`, `Map<K, V>`, `Set<T>` become usable through `from collections import ...` or as `collections.X` after `import collections`.
* Pure collection constructors (imported module): `collections.List.new()`, `collections.Map.new()`, `collections.Set.new()`, `collections.BitSet.new(size)`, `collections.Deque.new()`, `collections.MutableList.new()`, `collections.MutableList.from_list(xs)`, `collections.MutableMap.new()`, `collections.MutableMap.from_map(m)`, `collections.MutableMap.with_capacity(capacity)`, `collections.MutableSet.new()`, `collections.MutableSet.from_set(s)`, `collections.MutableSet.with_capacity(capacity)`, `collections.MutablePriorityQueue.new_min()`, `collections.MutablePriorityQueue.new_max()`, `collections.MutableBitSet.new(size)`, `collections.MutableBitSet.from_bitset(bs)`, `collections.MutableDeque.new()`, `collections.MutableDeque.from_deque(q)`
* Prelude traversal constructors (no import): `start..<end`, `seed.unfold(step)`.
* Pure no-owner utilities (imported module): `math.*`
* Effectful utilities (imported capability modules): `io.*`, `fs.*`
* Internal intrinsic IDs (`list_new`, `map_insert`, etc.) are implementation detail only.

Builtin types `Option<T>`, `Result<T, E>`, `ParseError`, and internal `Seq<T>` support are
injected into ambient scope before type-checking. Collection families are also injected with stable core identities, but their visible names live only under `collections` or explicit `from collections import ...` bindings. Synthetic modules (`collections`, `io`, `math`, `fs`) require explicit `import collections` / `import io` / `import math` / `import fs` in all modes.
Zero intrinsic free functions exist in user scope.

Runtime soundness invariants for core APIs:
* Core method/static dispatch is identity-based, not string-name-based. Core behavior binds to internal core type identity even when user types shadow names like `Result` or `List`.
* User type-name shadowing remains allowed (`type Result<...> = ...`, `type List<T> = ...`), but core APIs do not silently retarget.
* Constructor/pattern resolution is owner-based: `Type.Variant` is always valid, and bare `Variant` resolves only through explicit `from Type import Variant`.
* Core builtin constructor names are not globally reserved; user ADTs may define variants named `Some`, `None`, `Ok`, `Err`, `InvalidInt`, or `InvalidFloat` without colliding with builtin behavior.

Mental model: `List`/`Map`/`Set`/`BitSet`/`Deque` are immutable snapshot and value types, while `MutableList`/`MutableMap`/`MutableSet`/`MutableBitSet`/`MutableDeque`/`MutablePriorityQueue` are the canonical build and edit types. Collection families are not ambient globals; `collections` is the pure module namespace for their visible names, constructors, and explicit mutable-from-immutable conversions, and `to_list`/`to_map`/`to_set`/`to_bitset`/`to_deque` move back to immutable snapshots.
`io`/`fs` remain module namespaces for effectful operations.

**Implemented (v0.1+):**
* `Option<T>` — builtin ADT (`Some(T) | None`), used as return type for safe lookups ✓
  * Methods: `o.unwrap_or(fallback)`, `o.map_or(fallback, f)`, `o.map(f)`, `o.and_then(f)`
* `Result<T, E>` — builtin ADT (`Ok(T) | Err(E)`), `?` propagation works ✓
  * Methods: `r.unwrap_or(fallback)`, `r.map_or(fallback, f)`, `r.map(f)`, `r.and_then(f)`, `r.map_err(f)`
* `ParseError` — builtin ADT (`InvalidInt(String) | InvalidFloat(String)`), used as error type for `parse_int`/`parse_float` ✓
* `List<T>` — opaque builtin type with COW-backed persistent runtime storage (`Rc<Vec<Value>>`) ✓
  * Constructor: `collections.List.new()` (requires `import collections`)
  * Methods (query/value-transform): `xs.len()`, `xs.get(i)` → `Option<T>`, `xs.head()` → `Option<T>`, `xs.tail()`, `xs.is_empty()`, `xs.concat(ys)`, `xs.reversed()`, `xs.sorted()`, `xs.sorted_by(f)`, `xs.binary_search(x)`
  * Methods (traversal): `xs.map(f)`, `xs.filter(f)`, `xs.flat_map(f)`, `xs.scan(init, f)`, `xs.enumerate()`, `xs.zip(other)`, `xs.chunks(n)`, `xs.windows(n)`, `xs.fold(init, f)`, `xs.count()`, `xs.count(f)`, `xs.contains(value)`, `xs.frequencies()`, `xs.any(f)`, `xs.all(f)`, `xs.find(f)`, `xs.to_list()`
  * Immutable lists do not expose edit verbs such as `push`, `set`, or `update`.
  * Search helper: `xs.binary_search(x)` → `Int` with Rust/Java-style insertion contract:
    found index returns `>= 0`; missing element returns `-(insertion_point + 1)`.
    `insertion_point` is where `x` would be inserted to keep sorted order.
    Only naturally orderable element types are allowed (same as `xs.sorted()`).
* `MutableList<T>` — opaque builtin type with alias-visible mutable runtime storage ✓
  * Constructors: `collections.MutableList.new()` and `collections.MutableList.from_list(xs)` (requires `import collections`)
  * Methods (edit/query): `xs.push(v)`, `xs.insert(i, v)`, `xs.last()` → `Option<T>`, `xs.pop()` → `Option<T>`, `xs.extend(ys)` where `ys: List<T>`, `xs.len()`, `xs.head()` → `Option<T>`, `xs.tail()`, `xs.is_empty()`, `xs.get(i)` → `Option<T>`, `xs.set(i, v)`, `xs.delete_at(i)`, `xs.remove_at(i)` → `T`, `xs.update(i, f)`, `xs.reverse()`, `xs.sort()`, `xs.sort_by(f)`, `xs.binary_search(x)`, `xs[i]`, `xs.to_list()`
  * Indexed edit semantics: `insert` requires `0 <= i <= len`; `delete_at/remove_at/set/update` require `0 <= i < len`; out-of-bounds is a direct runtime error. Use `delete_at` for fluent deletion and `remove_at` when you need the removed value.
  * Methods (traversal): `xs.map(f)`, `xs.filter(f)`, `xs.flat_map(f)`, `xs.scan(init, f)`, `xs.enumerate()`, `xs.zip(other)`, `xs.chunks(n)`, `xs.windows(n)`, `xs.fold(init, f)`, `xs.count()`, `xs.count(f)`, `xs.contains(value)`, `xs.frequencies()`, `xs.any(f)`, `xs.all(f)`, `xs.find(f)`, `xs.to_list()`
  * Mutation semantics: updates are visible across aliases that reference the same `MutableList`.
* `MutableMap<K, V>` — opaque builtin type with alias-visible mutable key/value storage. Keys must satisfy `Hash + Eq`; invalid key types are rejected at compile time for typed mutable-map operations (E0024). For nominal keys, derive or implement those traits explicitly, e.g. `type Point derive(Eq, Hash) = { x: Int, y: Int }`. Primitive keys (`Int`, `String`, `Char`, `Bool`, `Unit`) use an internal ordered open-address fast path in mutable workloads; this is an implementation detail, not a separate surface type. ✓
  * Constructors: `collections.MutableMap.new()`, `collections.MutableMap.from_map(m)`, and `collections.MutableMap.with_capacity(capacity)` (requires `import collections`; capacity is a minimum live-element hint only)
  * Methods: `m.insert(k, v)`, `m.get(k)` → `Option<V>`, `m.get_or_insert_with(k, fn() => v)` → `V`, `m.contains(k)`, `m.remove(k)`, `m.len()`, `m.keys()`, `m.values()`, `m.is_empty()`, `m.to_map()`
* `MutableSet<T>` — opaque builtin type with alias-visible mutable set storage. Elements must satisfy `Hash + Eq`; invalid element types are rejected at compile time for typed mutable-set operations (E0028). For nominal elements, derive or implement those traits explicitly, e.g. `type Point derive(Eq, Hash) = { x: Int, y: Int }`. Primitive elements (`Int`, `String`, `Char`, `Bool`, `Unit`) use the same internal mutable fast path strategy as `MutableMap`; this remains an implementation detail. ✓
  * Constructors: `collections.MutableSet.new()`, `collections.MutableSet.from_set(s)`, and `collections.MutableSet.with_capacity(capacity)` (requires `import collections`; capacity is a minimum live-element hint only)
  * Methods: `s.insert(v)`, `s.contains(v)`, `s.remove(v)`, `s.len()`, `s.is_empty()`, `s.values()`, `s.to_set()`
* `MutablePriorityQueue<P, T>` — opaque builtin type with alias-visible prioritized worklist storage. Priorities must satisfy `Ord`; invalid priority types are rejected at compile time with missing-`Ord` diagnostics. ✓
  * Constructors: `collections.MutablePriorityQueue.new_min()`, `collections.MutablePriorityQueue.new_max()` (requires `import collections`; `P` and `T` are inferred from context or explicit annotation)
  * Methods: `pq.push(priority, value)`, `pq.peek()` → `Option<{ priority: P, value: T }>`, `pq.pop()` → `Option<{ priority: P, value: T }>`, `pq.len()`, `pq.is_empty()`
  * Semantics: `new_min()` returns the smallest priority first, `new_max()` returns the largest first, and equal-priority ties are returned in insertion order.
* `BitSet` — opaque builtin dense bitset with COW-backed packed-word runtime storage (`Rc<Vec<u64>>`). This is the canonical dense bounded-bit tool; `MutableList<Bool>` remains valid but is not the intended representation for dense relation/set workloads. ✓
  * Constructor: `collections.BitSet.new(size)` (requires `import collections`)
  * Domain: valid indices are `0..size-1`; `size` is fixed at construction; negative sizes are runtime errors.
  * Per-bit methods: `bs.test(i)`, `bs.with_bit(i)`, `bs.without_bit(i)`, `bs.toggled(i)`
  * Whole-set methods: `bs.union(other)`, `bs.intersection(other)`, `bs.difference(other)`, `bs.xor(other)`
  * Metadata/traversal: `bs.count()`, `bs.size()`, `bs.is_empty()`, `bs.values()` (ascending index order, lazy traversal)
  * Runtime errors: out-of-range indices and binary ops on mismatched sizes are direct runtime errors.
* `MutableBitSet` — opaque builtin dense bitset with alias-visible packed-word runtime storage (`Rc<RefCell<Rc<Vec<u64>>>>`). ✓
  * Constructors: `collections.MutableBitSet.new(size)` and `collections.MutableBitSet.from_bitset(bs)` (requires `import collections`)
  * Surface mirrors `BitSet` queries: `test/count/size/is_empty/values`
  * Per-bit mutation methods: `set/reset/flip`
  * Whole-set mutation methods: `union_with/intersection_with/difference_with/xor_with`
  * Conversion: `bs.to_bitset()`
  * Mutation semantics: updates and whole-set ops mutate in place, are alias-visible, and return the receiver for chaining.
* `Deque<T>` — opaque builtin type with COW-backed persistent runtime storage (`Rc<VecDeque<Value>>`) ✓
  * Constructor: `collections.Deque.new()` (requires `import collections`)
  * Methods (queue/storage): `q.prepended(v)`, `q.appended(v)`, `q.popped_front()` → `Option<{ value: T, rest: Deque<T> }>`, `q.popped_back()` → `Option<{ value: T, rest: Deque<T> }>`, `q.len()`, `q.is_empty()`
  * Methods (traversal): `q.map(f)`, `q.filter(f)`, `q.flat_map(f)`, `q.scan(init, f)`, `q.enumerate()`, `q.zip(other)`, `q.chunks(n)`, `q.windows(n)`, `q.fold(init, f)`, `q.count()`, `q.count(f)`, `q.contains(value)`, `q.frequencies()`, `q.any(f)`, `q.all(f)`, `q.find(f)`, `q.to_list()`
* `MutableDeque<T>` — opaque builtin type with alias-visible mutable queue storage ✓
  * Constructors: `collections.MutableDeque.new()` and `collections.MutableDeque.from_deque(q)` (requires `import collections`)
  * Methods (queue/storage): `q.push_front(v)`, `q.push_back(v)`, `q.pop_front()` → `Option<T>`, `q.pop_back()` → `Option<T>`, `q.len()`, `q.is_empty()`, `q.to_deque()`
  * Methods (traversal): `q.map(f)`, `q.filter(f)`, `q.flat_map(f)`, `q.scan(init, f)`, `q.enumerate()`, `q.zip(other)`, `q.chunks(n)`, `q.windows(n)`, `q.fold(init, f)`, `q.count()`, `q.count(f)`, `q.contains(value)`, `q.frequencies()`, `q.any(f)`, `q.all(f)`, `q.find(f)`, `q.to_list()`
* Traversal constructors and behavior (public surface) ✓
  * Half-open range source: `start..<end` (ascending, empty when `start >= end`)
  * Stateful source: `seed.unfold(step)` where `step: fn(S) -> Option<{ value: T, state: S }>`
  * Canonical user style is collection-first traversal (`xs.map(...).filter(...).count()` when the filtered traversal is reused; `xs.flat_map(fn(...) => ys)` for one-level flattening; `xs.count(f)` for direct predicate counts; `xs.contains(value)` for direct membership checks) on `List`, `MutableList`, `Deque`, and producer values (`String.split/lines/chars`, `Map.keys/values`, `Set.values`, ranges, unfolds)
  * Advanced adaptation: `flat_map` accepts callback results from `Seq`, `List`, `MutableList`, `Deque`, and nominal user types that implement `IntoTraversal<T>` via `IntoTraversal.into_seq(self)`.
* Supports transforms (`map/filter/flat_map/scan/enumerate/zip/chunks/windows`) and terminals (`fold/count()/count(f)/contains(value)/frequencies()/any/all/find/to_list`) with the same semantics as collection traversal
* Guidance: for predicate/search traversal, default to `s.any(f)`, `s.all(f)`, `s.find(f)`, `s.count(f)`, and `s.contains(value)`; use `s.frequencies()` for direct histogram/tally queries; reserve `s.fold(...)` for true accumulation/reduction
  * Evaluation model: each terminal re-runs the traversal pipeline from source (no single-use consumption state)
  * Implementation note: traversal is backed by an internal runtime/compiler engine type and is not nameable in user code
* `Map<K, V>` — opaque builtin type with witness-backed persistent runtime storage and deterministic insertion-order iteration. Keys must satisfy `Hash + Eq`; invalid key types are rejected at compile time for typed map operations (E0024). For nominal keys, derive or implement those traits explicitly, e.g. `type Point derive(Eq, Hash) = { x: Int, y: Int }`. `m.keys()` and `m.values()` return deterministic insertion order traversal values. Immutable maps are query/snapshot types; build and edit through `MutableMap` plus `to_map()`. ✓
  * Constructor: `collections.Map.new()` (requires `import collections`)
  * Methods: `m.get(k)` → `Option<V>`, `m.contains(k)`, `m.len()`, `m.keys()`, `m.values()`, `m.is_empty()`
* `Set<T>` — opaque builtin type with witness-backed persistent runtime storage and deterministic insertion-order iteration. Elements must satisfy `Hash + Eq`; invalid element types are rejected at compile time for typed set operations (E0028). For nominal elements, derive or implement those traits explicitly, e.g. `type Point derive(Eq, Hash) = { x: Int, y: Int }`. `s.values()` returns deterministic insertion-order traversal values. Immutable sets are query/snapshot types; build and edit through `MutableSet` plus `to_set()`. ✓
  * Constructor: `collections.Set.new()` (requires `import collections`)
  * Methods: `s.contains(v)`, `s.len()`, `s.is_empty()`, `s.values()`
* String methods ✓ — scalar-based (`s.len()` counts Unicode scalars; `s[i]`, `s.substring(a, b)`, and `s.chars()` operate on Unicode scalars), plus `s.contains(t)`, constrained-call-family `s.starts_with(t)` / `s.starts_with(t, start: idx)`, `s.ends_with(t)`, `s.trim()`, `s.split(sep)`, `s.to_upper()`, `s.to_lower()`, `s.concat(t)`, `s.lines()`, `s.parse_int()` → `Result<Int, ParseError>`, `s.parse_float()` → `Result<Float, ParseError>`
* Char methods ✓ — `c.to_string()`, `c.code()` (Unicode scalar / code point value; e.g. `let bucket = ch.code() % 256`), `c.is_decimal_digit()`, `c.to_decimal_digit() -> Option<Int>`, `c.to_digit(radix: Int) -> Option<Int>` where digit conversion uses ASCII `0-9` / `a-z` / `A-Z` semantics and `radix` must be in `2..=36`
* Int surface ✓ — native bitwise operators `&`, `|`, `^`, `~`, `<<`, `>>` (`Int` only, arithmetic `>>`, shift counts `0..63`) plus methods `n.abs()`, `n.pow(exp)` (`exp >= 0`, overflow checked), `n.to_string()`, `n.to_float()`
* Float methods ✓ — `f.abs()`, `f.to_int()`, `f.is_nan()`, `f.is_infinite()`, `f.is_finite()`
  * Float arithmetic intentionally follows IEEE behavior: `1.0 / 0.0` may produce `Infinity`, `-Infinity`, or `NaN`, and `1.0 % 0.0` yields `NaN`
  * Float comparisons also follow IEEE behavior: `NaN != NaN`, and ordered comparisons with `NaN` are `false`
  * `Float` is intentionally outside `Eq` / `Ord` / `Hash` in the phase-1 trait model because `NaN` breaks total equality and ordering laws
* Module-qualified math ✓ — `math.min(a, b)`, `math.max(a, b)`, `math.gcd(a, b)`, `math.lcm(a, b)`, `math.fmin(a, b)`, `math.fmax(a, b)`
* Module-qualified I/O ✓ — `io.print(s)`, `io.println(s)`, `io.read_line()`, `io.read_stdin()` (require `io` capability)
* Module-qualified filesystem ✓ — `fs.read_file(path)` (requires `fs` capability)

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
* Intrinsic functions via canonical API: module-qualified (`io.println`, `io.print`), methods (`n.to_string()`, `s.concat(t)`) ✓
* Builtin `Option<T>` and `Result<T, E>` types (injected as synthetic ADTs; `?` works out of the box) ✓
* Canonical formatter (`kyokara fmt`, `kyokara-fmt` crate, Wadler-Lindig Doc IR) ✓
* Stable symbol IDs (`kind::name` / `kind::parent::child` format, unique across symbol kinds) ✓
* Runtime contract checks (requires/ensures/old) ✓
* Core stdlib (List, MutableList, MutableMap, MutableSet, MutablePriorityQueue, Deque, Map, Set, BitSet, MutableBitSet, String, Int/Float, io, math, hash, fs — intrinsic functions exposed via canonical method/module API) ✓

**v0.2 — Refactoring + LSP + Capabilities**
* Module system: convention-based file layout, `pub` visibility, namespace imports + `from ... import ...` member imports ✓
* Refactor engine: rename symbol (single-file + multi-file), add missing match cases, add missing capability annotation ✓ — CST-based, post-refactor verification, structured TextEdit patches
* Refactor transactions: atomic refactor operations with in-memory re-check ✓ — `transact()` / `transact_project()` apply edits, re-run the type checker, and return `VerificationStatus` (Verified / Failed / Skipped). CLI gates `--apply` on verification passing; `--force` bypasses. API returns `"typechecked"` / `"failed"` / `"skipped"` status with structured verification diagnostics (message, code, span). Quickfix actions accept `--target-file` to disambiguate which module an offset refers to in project mode. CLI auto-detects project mode for `main.ky` with sibling `.ky` files; `--project` flag forces project mode for other entry files.
* LSP server: salsa incrementality, diagnostics, hover, go-to-definition, find references, completion, code actions (quickfixes), formatting ✓
* Capability enforcement: type-level checking (E0011) ✓ + runtime manifest enforcement (`--caps`, deny-by-default) ✓ (fine-grained fields currently fail closed with `UnsupportedManifestConstraint`)

**v0.3 — Verification + Codegen + Replay**
* Property-based test harness ✓ (`pbt` crate: choice-sequence engine, type-driven generators with `Gen.*` specs and `<-` bindings, 4-pass shrinker, corpus persistence, `where`-constrained generation via discard budgets; `kyokara test <file> --explore` discovers contract functions and explicit `property` declarations, generates random inputs, checks contracts/properties, shrinks counterexamples)
* SMT integration for contract verification (restricted fragment: linear arithmetic + uninterpreted functions, best-effort, never blocks compilation)
* KyokaraIR data structures ✓ (SSA, block params, text format, validator) + HIR→KIR lowering ✓ + WASM backend ✓ (public run/build/replay for single-file + project/package mode, frozen-surface parity across strings, closures, collections, trait-backed builtins, and shared replay/capability host ABI)
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
* explicit caps -> `net`, `db`, `secrets`, `clock`
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
11. ~~Implement module system (convention-based layout, pub visibility, namespace imports + `from ... import ...` member imports).~~ ✓
12. ~~Implement LSP server with salsa incrementality (diagnostics, hover, goto-def, references, completion, code actions, formatting).~~ ✓
13. ~~Implement WASM runtime host functions for capabilities + replay log.~~ ✓
14. ~~Add property test runner and basic generators.~~ ✓
15. Integrate SMT solver for opt-in static verification.

---

## 16. AI-first feature tracker

This section is the canonical AI-feature status tracker (migrated from
`docs/ai-programmer-special-features-tracker.md`) so design intent and status
are maintained in one place.

### 16.1 Completeness rubric

| Score | Meaning |
|---|---|
| `0%` | Not started; only problem statement exists. |
| `25%` | Design/intent captured in issues, little to no implementation. |
| `50%` | Partial implementation exists but core gaps remain. |
| `75%` | Functionally present, but hardening/docs/coverage still incomplete. |
| `100%` | Implemented, tested, and docs aligned with behavior. |

### 16.2 Wishlist features (AI-native)

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
| `W11` | Deterministic canonical formatter (predictable edits) | Implemented | `95%` | [#34](https://github.com/kyokaralang/kyokara/issues/34), [#404](https://github.com/kyokaralang/kyokara/issues/404) |
| `W12` | Typed serialization contracts + schema evolution policy | Missing | `5%` | [#235](https://github.com/kyokaralang/kyokara/issues/235) |
| `W13` | Partial-program checking (typed holes + incomplete code support) | Implemented | `100%` | [#35](https://github.com/kyokaralang/kyokara/issues/35) |
| `W14` | Fast incremental compiler query API (low-latency AI edit loops) | Partial/weak | `35%` | [#33](https://github.com/kyokaralang/kyokara/issues/33), [#239](https://github.com/kyokaralang/kyokara/issues/239) |
| `W15` | Constrained metaprogramming policy (typed/hygienic/inspectable if added) | Deferred | `20%` | [#242](https://github.com/kyokaralang/kyokara/issues/242) |

### 16.3 Documented AI-beneficial features not in wishlist

| ID | Additional documented feature | Assessment | Completeness | GitHub issue(s) |
|---|---|---|---|---|
| `D1` | Contracts as first-class syntax (`requires`/`ensures`/`old`) | Implemented + ongoing verification work | `80%` | [#9](https://github.com/kyokaralang/kyokara/issues/9), [#28](https://github.com/kyokaralang/kyokara/issues/28), [#30](https://github.com/kyokaralang/kyokara/issues/30) |
| `D2` | Property-based testing integrated in language workflow | Implemented + expansion ongoing | `85%` | [#23](https://github.com/kyokaralang/kyokara/issues/23), [#200](https://github.com/kyokaralang/kyokara/issues/200), [#25](https://github.com/kyokaralang/kyokara/issues/25) |
| `D3` | Deterministic replay logging/execution for auditability | Implemented (interpreter runtime) | `85%` | [#26](https://github.com/kyokaralang/kyokara/issues/26), [#27](https://github.com/kyokaralang/kyokara/issues/27) |
| `D4` | Refactor transactions with verify-before-apply behavior | Implemented | `90%` | [#32](https://github.com/kyokaralang/kyokara/issues/32), [#190](https://github.com/kyokaralang/kyokara/issues/190), [#191](https://github.com/kyokaralang/kyokara/issues/191) |
| `D5` | Compiler-as-API outputs for AI loops (diagnostics/symbol graph/holes + optional typed AST) | Implemented with dual-mode contract (RFC 0007); schema versioning pending | `90%` | [#36](https://github.com/kyokaralang/kyokara/issues/36), [#38](https://github.com/kyokaralang/kyokara/issues/38), [#241](https://github.com/kyokaralang/kyokara/issues/241), [#235](https://github.com/kyokaralang/kyokara/issues/235) |
| `D6` | LSP support for interactive coding loops | Implemented baseline, stronger incrementality pending | `70%` | [#33](https://github.com/kyokaralang/kyokara/issues/33), [#239](https://github.com/kyokaralang/kyokara/issues/239) |
| `D7` | API surface law (canonical placement/order/pipe compatibility for AI generation) | Implemented (core) + hardening follow-ups | `90%` | [#243](https://github.com/kyokaralang/kyokara/issues/243), [#265](https://github.com/kyokaralang/kyokara/issues/265), [#266](https://github.com/kyokaralang/kyokara/issues/266), [#267](https://github.com/kyokaralang/kyokara/issues/267), [#293](https://github.com/kyokaralang/kyokara/issues/293), [#236](https://github.com/kyokaralang/kyokara/issues/236), [#238](https://github.com/kyokaralang/kyokara/issues/238) |

### 16.4 Active docs-vs-implementation drift

| ID | Drift item | Assessment | Completeness | GitHub issue(s) |
|---|---|---|---|---|
| `X1` | Replay CLI is documented as available in multiple docs, but runtime/CLI path is not fully exposed | Open drift | `30%` | [#240](https://github.com/kyokaralang/kyokara/issues/240), [#26](https://github.com/kyokaralang/kyokara/issues/26), [#27](https://github.com/kyokaralang/kyokara/issues/27) |
| `X2` | `typed_ast` contract drift between docs and API output | Resolved via RFC 0007 dual-mode output (`typed_ast` opt-in) | `100%` | [#241](https://github.com/kyokaralang/kyokara/issues/241), [#39](https://github.com/kyokaralang/kyokara/issues/39) |

### 16.5 Update protocol

1. When an issue state changes, update the matching row's assessment and completeness.
2. If work lands without an issue link, create one and add it to the row.
3. If docs and implementation diverge, add or update a drift row in 16.4.
4. Keep this section in sync with `README.md`.

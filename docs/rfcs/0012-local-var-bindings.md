# RFC 0012: Local Mutable Bindings (`var`)

- Status: Draft
- Owner: Language Design
- Tracking issue: TBD
- Last updated: 2026-03-07

## Summary

Add a narrow local mutability surface to Kyokara:

1. `var x = expr` introduces a mutable local binding.
2. `x = expr` reassigns an existing mutable local binding.
3. `let x = expr` remains the default immutable binding form.

This RFC is intentionally small:

1. `var` is local-only.
2. Assignment targets are bare local identifiers only.
3. No field assignment.
4. No index assignment.
5. No top-level `var`.
6. No change to immutable-by-default collection policy.

## Motivation

Kyokara already committed to statement loops (`for` / `while`) for hot-path and AI-generated algorithm code via RFC 0006, but loop-carried state still lacks a first-class surface.

Today, iterative kernels often emulate local reassignment with one-slot mutable wrappers:

```kyokara
let current = collections.MutableList.new().push(start)
while (true) {
  let next = ...
  let _c = current.set(0, next)
}
```

This is semantically workable but poor as a canonical language surface:

1. It obscures intent: the algorithm wants a changing local, not a mutable list.
2. It adds container ceremony unrelated to the actual problem.
3. It reduces AI pass@1 by forcing synthetic wrapper patterns.
4. It makes imperative loop examples in docs noisier than they need to be.

The motivating Day 24 BFS pattern is:

```kyokara
var current = start_frontier
var time = start_t

while (true) {
  time = time + 1
  let next = ...
  current = next.to_list()
}
```

That is clearer than the current one-slot `MutableList` workaround and matches the intent of RFC 0006 more directly.

## Design Goals

1. Make loop-carried state explicit and ergonomic.
2. Keep immutable bindings (`let`) as the default.
3. Minimize grammar and semantic surface area.
4. Preserve deterministic, easy-to-explain local semantics for AI and humans.
5. Avoid turning Kyokara into a broadly mutable language.

## Non-Goals

1. Mutable fields or object properties.
2. Mutable array/index assignment syntax like `xs[i] = v`.
3. Top-level mutable globals.
4. Mutable function parameters.
5. Changing collection default policy from immutable-first to mutable-first.
6. Solving closure-captured mutable locals with implicit hidden cells in v1.

## Proposal

### P1. Syntax

Add one new keyword and one new statement form:

```kyokara
var x = expr
var x: T = expr
x = expr
```

Rules:

1. `var` requires an initializer.
2. Type annotations are allowed in v1: `var x: T = expr`.
3. Assignment is statement-only.
4. Assignment target must be a bare identifier.
5. `let` remains immutable and unchanged.

Examples:

```kyokara
fn sum_odds(n: Int) -> Int {
  var acc = 0
  for (x in 0..<n) {
    if ((x % 2) == 0) { continue }
    if (x > 1000) { break }
    acc = acc + x
  }
  acc
}
```

```kyokara
fn bfs(...) -> Int {
  var frontier = start_frontier
  var time = start_t
  while (true) {
    time = time + 1
    let next = collections.MutableList.new()
    ...
    frontier = next.to_list()
  }
}
```

### P2. Scope and shadowing

`var` follows the same lexical scope model as `let`.

Rules:

1. `var` bindings are local to the block/function where declared.
2. Existing shadowing rules remain unchanged.
3. Assignment resolves to the nearest in-scope binding with that name.
4. If that binding is immutable, assignment is an error even if an outer mutable binding exists.

Example:

```kyokara
fn demo() -> Int {
  var x = 1
  {
    let x = 2
    x = 3  // error: inner x is immutable
  }
  x
}
```

### P3. Mutation model

`var` is about mutable local bindings, not mutable values.

This means:

1. `var x = expr` creates a reassignable local slot.
2. Reassignment changes which value the local name refers to.
3. Values themselves keep their existing semantics:
   - `List`, `Map`, `Set`, `BitSet`, `Deque` stay immutable values
   - `MutableList`, `MutableMap`, `MutableSet`, `MutableBitSet` stay explicit mutable container types

Annotated mutable bindings are also allowed:

```kyokara
var frontier: List<Int> = start_frontier
var time: Int = start_t
```

So this remains valid and meaningful:

```kyokara
var xs = collections.List.new()
xs = xs.push(1)
xs = xs.push(2)
```

This RFC does **not** invert the immutable-default collection model.

### P4. Closure capture rule (v1)

To keep the first version narrow and unsurprising, mutable locals may not be captured by nested functions or lambdas.

Example:

```kyokara
fn bad() -> fn() -> Int {
  var x = 1
  fn() -> Int => x  // error in v1
}
```

Rationale:

1. It avoids implicit hidden cell semantics in the first design pass.
2. It keeps local mutation easy to reason about.
3. It covers the main motivating use case: loop-carried state within one function.

This can be revisited later if closure-captured mutable locals prove necessary.

### P5. Target restrictions

Only local identifier assignment is introduced:

Allowed:

```kyokara
x = y + 1
```

Not added by this RFC:

```kyokara
obj.field = 1
xs[i] = 2
foo().bar = 3
```

Those remain out of scope to preserve a small, explicit surface.

## Diagnostics

Targeted diagnostics should be direct and non-cascading:

1. `assignment target must be a local variable`
2. `` `x` is immutable; use `var` if rebinding is intended ``
3. `` `var` requires an initializer ``
4. `` top-level `var` bindings are not allowed ``
5. `` mutable locals cannot be captured by nested functions or lambdas `` (v1 policy)

Public diagnostic coding can reuse existing parse/type diagnostic categories; no schema expansion is required at RFC level.

## Runtime and Compiler Notes

Expected implementation shape:

1. Parser:
   - add `var` keyword
   - add `VarBinding`
   - add assignment statement parsing
2. HIR:
   - track local mutability
   - represent assignment as dedicated statement/expr node
3. Type checking:
   - enforce mutable-target rule
   - reject assignment to immutable locals / params / imports / top-level names
   - reject captured mutable locals in v1
4. Eval:
   - represent mutable locals as assignable slots in the environment
   - reuse existing lexical scope and slot-resolution machinery

This RFC is intentionally compatible with the recent local-slot work: mutable locals should lower to direct slot updates rather than name-based reassignment.

## AI-First Rationale

This RFC improves AI generation more directly than the current one-slot mutable-wrapper pattern:

1. `var` / `x = ...` is a widely learned pattern.
2. It removes accidental container-shaped workarounds from algorithmic code.
3. It aligns the loop surface with the intent of RFC 0006.
4. It reduces the gap between what an AI naturally writes for BFS/DP/worklists and what Kyokara currently accepts.

## RFC Alignment

### RFC 0001 (API Surface Law)

No conflict. This RFC adds syntax for local state, not duplicate APIs.

### RFC 0005 / 0008 / 0009 / 0010 (Mutable collections)

Complementary, not contradictory.

Those RFCs standardize:

1. immutable collections as the default nouns
2. explicit `Mutable*` collection families for alias-visible container mutation

This RFC keeps that model intact.

`var` solves a different problem:

1. loop-carried local state
2. scalar accumulators
3. whole-value rebinding across iterations

### RFC 0006 (Loop control)

This RFC completes the ergonomic story that RFC 0006 currently leaves open.

`for` / `while` provide imperative iteration shape.
`var` provides imperative local-state evolution without synthetic wrapper cells.

## Alternatives Considered

### A1. Keep wrapper-cell pattern only

Examples:

1. one-slot `MutableList<Int>` for counters
2. one-slot `MutableList<List<T>>` for frontier swapping

Pros:

1. No new syntax.

Cons:

1. Poor readability.
2. Lower AI pass@1.
3. Makes the language look more awkward than it needs to.

Decision: reject as canonical surface.

### A2. Add a loop-state / `recur` form instead of `var`

Pros:

1. Preserves immutable local bindings.
2. Makes loop-carried state explicit in a functional style.

Cons:

1. Less familiar to most AI systems and most programmers.
2. Larger conceptual jump from the current `for` / `while` surface.
3. Harder to explain alongside already-shipped imperative loops.

Decision: defer.

### A3. Make mutable collections the default and immutable ones special

Pros:

1. Imperative code would become shorter in some cases.

Cons:

1. Solves the wrong problem: collection default mutability is not the same as local rebinding.
2. Conflicts with RFC 0005 / 0008 / 0009 / 0010, which already committed to immutable defaults plus explicit `Mutable*` tools.
3. Increases aliasing and reasoning cost across the language surface.
4. Reduces the current “immutable by default, mutable when chosen” predictability story.

Decision: reject.

### A4. Allow assignment to fields and indexes immediately

Pros:

1. More imperative familiarity.

Cons:

1. Much larger semantic surface.
2. Blurs the current distinction between immutable values and explicit mutable containers.
3. Harder to reason about and implement correctly in one pass.

Decision: reject for v1.

## Rollout

1. Ratify local-only `var` and bare-name assignment.
2. Implement parser/lowering/type/eval support.
3. Add regression tests for:
   - loop-carried scalar state
   - frontier swapping
   - immutable assignment rejection
   - shadowing interactions
   - closure-capture rejection
4. Update docs/examples to replace one-slot `MutableList` accumulator examples where `var` is clearer.

## Acceptance Criteria

1. `var x = expr` works for local mutable bindings.
2. `x = expr` works when `x` is a mutable local in scope.
3. Assignment to `let` locals is rejected with a targeted diagnostic.
4. Assignment to fields/indexes remains unsupported.
5. Existing immutable collection defaults remain unchanged.
6. `var x: T = expr` is accepted and type-checks like `let x: T = expr`, but with mutable-local semantics.
7. RFC 0006 loop examples can be expressed without one-slot mutable wrapper cells where only local rebinding is needed.

## Open Questions

1. Should `for` loop binders ever allow a mutable form (`for (var x in xs)`) or should iteration binders remain immutable-only?
2. If closure-captured mutable locals are later needed, should they use explicit cell semantics or implicit capture-by-reference semantics?

# RFC 0003: Opaque Traversal Constructor Surface (`..<` + `.unfold`, no public `Seq`)

- Status: Accepted
- Owner: Language Design
- Tracking issue: TBD
- Last updated: 2026-03-04

## Summary

Hide traversal engine types from user surface completely.

1. Integer range constructor is expression syntax: `start..<end`.
2. Stateful generator constructor is universal method syntax: `seed.unfold(step)`.
3. Public `Seq` constructor/type surface is removed (`Seq.range`, `Seq.unfold`, `Seq<T>` are invalid in user code).

Traversal remains lazy and re-iterable internally via the existing runtime/compiler traversal plan.

## Motivation

After RFC 0002, traversal became collection-first for method chains, but constructor/type leakage still existed:

1. Users could still call `Seq.range(...)` and `Seq.unfold(...)`.
2. Users could still write `Seq<T>` in signatures.
3. This kept a mixed mental model and introduced unnecessary boundary mistakes.

v0 design goal is a minimal, obvious, and predictable surface for AI generation.

## Scope

In scope:

1. Range constructor syntax (`..<`) in lexer/parser/fmt/type/runtime/lowering.
2. Universal `.unfold(step)` method surface.
3. Removal of public `Seq` constructor and annotation surface.
4. Docs/completions/diagnostics alignment to opaque traversal surface.

Out of scope:

1. Traversal runtime redesign/fusion.
2. New traversal operations beyond constructor changes.
3. Migration hints (v0 hard break policy).

## Proposal

### P1. Range constructor syntax

Add infix expression `start..<end` with half-open ascending semantics:

1. Includes `start`.
2. Excludes `end`.
3. Returns empty traversal when `start >= end`.

Precedence is above pipeline and below arithmetic/logical operators to keep expressions predictable.

### P2. Universal unfold constructor

Expose `unfold` as a method on any seed value:

```kyokara
seed.unfold(fn(state: S) => Option<{ value: T, state: S }>)
```

Semantics are unchanged:

1. `Option.None` stops.
2. `Option.Some({ value, state: next })` emits `value` and continues with `next`.

### P3. Remove public `Seq` surface

Hard breaks:

1. `Seq.range(...)` rejected.
2. `Seq.unfold(...)` rejected.
3. `Seq<T>` type annotation rejected.

Internal `Seq` identity remains implementation detail only.

### P4. User-facing hygiene

1. Diagnostics should not require users to reason about public `Seq`.
2. Completion/symbol outputs should not advertise internal `$core_*` names.
3. Docs/examples must use only `..<` and `.unfold(step)` for traversal construction.

## Examples

Before:

```kyokara
let xs = Seq.range(0, 5)
let ys = Seq.unfold(0, step)
fn f(s: Seq<Int>) -> Int { s.count() }
```

After:

```kyokara
let xs = (0..<5)
let ys = (0).unfold(step)
// Traversal is not a nameable public type.
```

## Acceptance Criteria

1. Canonical construction is exactly `..<` and `.unfold(step)`.
2. Public `Seq.*` and `Seq<T>` are rejected.
3. Existing traversal laziness and short-circuit behavior remain unchanged.
4. Docs/examples/completions/diagnostics reflect the opaque traversal model.

## Alternatives Considered

### A1. Constructor namespace (`Iter.range`, `Iter.unfold`)

Pros:

1. Keeps constructors namespaced and explicit.
2. Hides `Seq` from user surface.

Cons:

1. Adds another public surface symbol (`Iter`) with little additional power.
2. Still forces constructor ownership vocabulary in simple range cases.

Decision: rejected in favor of `start..<end` and universal `.unfold(step)`.

### A2. Global free functions (`range`, `unfold`)

Pros:

1. Shortest spelling.

Cons:

1. Conflicts with API surface law direction (avoid free-function runtime APIs).
2. Weak ownership/discoverability compared with method/syntax-based constructors.

Decision: rejected.

### A3. Keep `Seq.range` / `Seq.unfold` and `Seq<T>` public

Pros:

1. No churn.

Cons:

1. Preserves constructor/type leakage of internal traversal engine names.
2. Keeps known AI pass@1 boundary confusion.

Decision: rejected.

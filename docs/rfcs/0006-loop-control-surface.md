# RFC 0006: Loop Control Surface for Hot Paths (`for` / `while`)

- Status: Implemented
- Owner: Language Design
- Tracking issue: #363 (remaining KIR follow-up only)
- Last updated: 2026-03-08

## Summary

Add a minimal imperative loop surface to Kyokara for performance-sensitive kernels and clearer AI generation:

1. `while (condition) { ... }`
2. `for (item in xs) { ... }`
3. `break` and `continue`

`for (item in xs)` accepts any traversable source, including:

1. Ranges (`start..<end`)
2. Collection receivers (`List`, `MutableList`, `Deque`)
3. Producer values (`String.lines/chars/split`, `Map.keys/values`, `Set.values`)
4. Traversal pipelines (`map/filter/zip/chunks/windows/...`)
5. Stateful generators (`seed.unfold(step)`)

This RFC is additive and keeps existing combinator APIs (`map/filter/fold/...`) intact.

Implementation note:

1. The core loop surface is shipped on `main`.
2. RFC 0013 now supplies the local mutable-binding surface (`var` / `x = expr`) that completes the loop-carried-state story for imperative kernels.
3. `#363` tracks only a remaining KIR lowering follow-up, not the source-language loop surface itself.

## Motivation

AoC-style dense kernels still show a runtime gap versus imperative equivalents, even after `MutableList` introduction, because the current runtime executes many hot loops through closure-heavy traversal combinators.

Goals:

1. Improve runtime behavior in hot loops on the current tree-walking runtime.
2. Improve pass@1 for AI generation on algorithmic tasks.
3. Keep syntax small and unambiguous.

## Design Goals

1. Minimal loop surface, no feature explosion.
2. Unambiguous syntax with mandatory parentheses/braces.
3. Consistent with collection-first traversal model.
4. Preserve existing functional combinators as first-class style for non-hot code.
5. Deterministic semantics suitable for AI and tooling.

## Non-Goals

1. List comprehensions.
2. Labeled break/continue.
3. C-style `for(init; cond; step)` syntax.
4. Implicit iterator protocol redesign.
5. Removing combinator APIs.

## Proposal

### P1. Syntax

```kyokara
while (cond) {
  body
}

for (x in xs) {
  body
}
```

Rules:

1. Parentheses are mandatory at loop heads.
2. Braced block is mandatory.
3. `in` is a reserved loop keyword in this context.

### P2. Traversable domain for `for`

`xs` must type-check as a traversable source under the existing traversal compatibility rules.

Canonical examples:

```kyokara
for (i in 0..<n) { ... }
for (line in text.lines()) { ... }
for (k in m.keys()) { ... }
for (v in s.values()) { ... }
for (x in seed.unfold(step)) { ... }
for (p in points.map(f).filter(g)) { ... }
```

### P3. Control flow

1. `break` exits nearest loop.
2. `continue` skips to next iteration of nearest loop.
3. `return` behavior is unchanged (exits function).

### P4. Value semantics

1. Loop constructs are statements with `Unit` result.
2. They are valid inside block statement positions.
3. They are not expression-valued forms.

### P5. Evaluation semantics

For `for (item in xs) { body }`:

1. Evaluate `xs` once at loop entry.
2. Iterate in traversal order.
3. Bind `item` per iteration as loop-local binding.
4. Respect `break`/`continue`/`return` control flow.

For `while (cond) { body }`:

1. Re-evaluate `cond` each iteration.
2. `cond` must be `Bool`.

### P6. Diagnostics

Targeted parse/type diagnostics should be direct and non-cascading:

1. `while condition must be parenthesized`
2. `for loop head must be parenthesized`
3. `for loop requires 'in'`
4. `for source must be traversable`
5. `` `break` used outside loop ``
6. `` `continue` used outside loop ``

No migration-hint wording is required.

## Runtime and Compiler Notes

This RFC does not require removing or deprecating combinators.

Expected implementation strategy:

1. Lower loops to dedicated HIR forms.
2. Interpreter executes loops directly without per-element user closure dispatch unless body itself calls closures.
3. `for` over traversables reuses internal traversal plan machinery but avoids extra `fold` callback layers.

This gives a practical hot-path speedup without waiting for global fusion/JIT infrastructure.

## AI-First Rationale

1. Most AI systems strongly expect `for`/`while` constructs in algorithmic code.
2. Explicit loop intent reduces generation ambiguity in dense update kernels.
3. Keeping both paradigms (`for`/`while` and combinators) allows intent-driven choice:
   - combinators for declarative transforms
   - loops for kernel-style updates and early exits

## RFC Alignment

### RFC 0001 (API Surface Law)

No conflict. This RFC adds syntax/control flow, not duplicate API synonyms.

### RFC 0002/0003 (Traversal Surface)

No conflict. `for` consumes the same traversable model already exposed via collection-first traversal and traversal constructors (`..<`, `.unfold(...)`).

### RFC 0005 (Mutable naming/placement)

Complementary. `MutableList` is still the explicit mutable tool for alias-visible container mutation; loops make hot-path iteration ergonomic without changing collection default mutability.

### RFC 0013 (Local mutable bindings)

Complementary. RFC 0013 adds `var` and bare-name reassignment so loop-carried local state no longer needs one-slot mutable wrapper cells.

## Alternatives Considered

### A1. Keep combinators only

Pros:

1. Smaller syntax.

Cons:

1. Harder to optimize hot kernels in current interpreter model.
2. Lower pass@1 for AI on imperative algorithm patterns.

Decision: reject.

### A2. Add only `for`, no `while`

Pros:

1. Smaller surface.

Cons:

1. Awkward for state-driven loops with non-traversal stop conditions.

Decision: reject.

### A3. Add C-style `for(init; cond; step)`

Pros:

1. Familiar in some ecosystems.

Cons:

1. Larger grammar surface.
2. More parser and tooling complexity.

Decision: reject.

## Rollout

1. Implement parser/lowering/type/eval support for `while`, `for-in`, `break`, `continue`.
2. Add strict diagnostics and recovery tests to prevent cascades.
3. Add formatter rules with canonical parenthesized heads and braced bodies.
4. Update docs/examples to show when loops are preferred for hot kernels.

## Acceptance Criteria

1. `for (x in xs)` works for all existing traversable sources, including ranges.
2. `while (cond)` works with strict `Bool` condition checking.
3. `break`/`continue` function correctly and error cleanly outside loops.
4. Existing combinator code remains valid and behaviorally unchanged.
5. Day 08-style kernel rewrites can be expressed without combinator acrobatics.

## Open Questions

1. Should there be a future `loop { ... }` form for explicit infinite loops?
2. Should formatter enforce single-line compact loop heads only, or allow multiline head expressions?

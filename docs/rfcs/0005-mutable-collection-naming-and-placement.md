# RFC 0005: Mutable Collection Naming and Placement

- Status: Draft
- Owner: Language Design
- Tracking issue: TBD
- Last updated: 2026-03-05

## Summary

Define one explicit policy for mutable collection APIs:

1. Mutable collection type names use full `Mutable*` form (`MutableList`, `MutableMap`, etc.).
2. Mutable collections live under the pure `collections` module namespace.
3. Prelude collection names remain immutable by default (`List`, `Map`, `Set`).
4. Mutability is never implied by alternate nouns like `Array`.

This RFC is about naming and placement, not full mutability semantics per type.

## Motivation

Recent AoC work highlighted a naming and expectation gap:

1. A type named `Array` can be misread as "the mutable dense one" even when semantics are persistent/COW.
2. Abbreviated names like `MutList` are compact but less predictable for AI generation.
3. Global mutable constructors would increase prelude noise and reduce pass@1 consistency.

Kyokara needs one deterministic rule for where mutable structures live and how they are named.

## Design Goals

1. Make mutability explicit at the type name level.
2. Keep immutable default ergonomics unchanged.
3. Preserve a small prelude surface.
4. Keep module and capability mental model consistent with RFC 0004.
5. Optimize for AI predictability over brevity.

## Non-Goals

1. Defining exact operational semantics for every mutable collection in this RFC.
2. Introducing capability gates for pure mutable collections.
3. Deciding all future mutable collection APIs in one pass.

## Proposal

### P1. Naming rule

Use `Mutable*` as the canonical mutable collection prefix.

Examples:

1. `MutableList`
2. `MutableMap`
3. `MutableSet`
4. `MutableDeque`
5. `MutableArray` (if needed)

Rejected style:

1. `MutList`
2. `MList`
3. Reusing immutable names for mutable behavior (`List` becoming mutable)

Rationale: full words reduce ambiguity for both humans and AI agents.

### P2. Placement rule

Mutable collection constructors are module-qualified under `collections`.

Canonical form:

1. `import collections`
2. `collections.MutableList.new(...)`

Properties:

1. Pure API: import required, no `with`, no manifest grant.
2. Type remains nameable in annotations (`MutableList<Int>`) unless a future RFC changes type-surface policy.
3. Constructor is not prelude-global.

### P3. Immutable/mutable split

Keep prelude immutable defaults:

1. `List`, `Map`, `Set` stay immutable value types.

Add explicit mutable counterparts in `collections`:

1. `collections.MutableList` first.
2. Other mutable structures (`MutableArray`, `MutableDeque`, etc.) only when justified by concrete workloads.

Important:

1. `Array` must not be treated as the mutability signal.
2. If `Array` exists, its mutability semantics are independent and must be documented explicitly.

### P4. Canonical examples

```kyokara
import collections

fn demo() -> Int {
  let xs = collections.MutableList.new().push(1).push(2)
  xs.set(0, 9)
  xs.get(0).unwrap_or(0)
}
```

```kyokara
import collections

fn freeze_example() -> List<Int> {
  let xs = collections.MutableList.new().push(3).push(4)
  xs.to_list()
}
```

## RFC Alignment

### RFC 0001 (API Surface Law)

This RFC strengthens API predictability by making mutability explicit in names and avoiding shorthand ambiguity.

### RFC 0004 (Module Taxonomy)

This RFC applies the same placement model:

1. Specialized pure APIs are module-qualified under `collections`.
2. Mutability does not imply capability authority.

## Rollout

v0 default rollout policy:

1. Introduce mutable collections with `Mutable*` names in `collections`.
2. Keep immutable prelude collections unchanged.
3. Do not add migration-hint diagnostics by default.

## Alternatives Considered

### A1. Use short prefix (`Mut*`)

Pros:

1. Shorter call sites.

Cons:

1. Higher ambiguity.
2. Lower AI predictability.

Decision: reject.

### A2. Keep mutable collections in prelude

Pros:

1. Fewer imports.

Cons:

1. Larger global surface.
2. Weaker signal of specialized/performance-oriented APIs.

Decision: reject.

### A3. Use `Array` as mutable default

Pros:

1. Familiar to some language ecosystems.

Cons:

1. Conflicts with existing expectation that `Array` may be immutable/fixed-size.
2. Creates naming mismatch instead of resolving it.

Decision: reject.

## Acceptance Criteria

1. Mutable collection naming guidance is explicit and documented (`Mutable*` only).
2. Placement guidance is explicit and documented (`collections.*` constructors).
3. Docs consistently present immutable prelude defaults plus explicit mutable module types.

## Open Questions

1. Should `MutableList` ship before `MutableArray`, or should both land together?
2. Should mutable method return conventions be standardized (`Unit` vs chaining) in a follow-up RFC?

# RFC 0005: MutableList Naming and Placement

- Status: Draft
- Owner: Language Design
- Tracking issue: TBD
- Last updated: 2026-03-05

## Summary

Define the v0 policy for introducing mutability without over-expanding the surface:

1. First mutable collection is `MutableList`.
2. Mutable collection names use the explicit `Mutable*` prefix.
3. Mutable collection constructors live under the pure `collections` module.
4. `Array` is not the mutability signal and is not part of this RFC scope.

This RFC is intentionally narrow: lock naming/placement now, ship `MutableList` first, and defer additional mutable collection types to follow-up RFCs.

## Motivation

Recent AoC workloads need dense index updates with less state-threading overhead than persistent `List.set` loops.

Two mistakes we want to avoid:

1. Ambiguous naming (`MutList`, `MList`) that reduces pass@1 predictability for AI generation.
2. Implying mutability through alternate nouns (`Array`) instead of an explicit type signal.

Kyokara needs one deterministic rule: mutability is explicit in the type name and module placement.

## Design Goals

1. Keep immutable defaults untouched (`List`, `Map`, `Set`).
2. Add one obvious mutable tool for performance-sensitive loops.
3. Keep prelude small; put specialized structures behind `import collections`.
4. Preserve capability model consistency: pure collection APIs require no capability grant.
5. Maximize AI predictability with explicit, regular naming.

## Non-Goals

1. Defining all mutable collections in one RFC.
2. Introducing `MutableArray`, `MutableDeque`, or `MutableMap` in this change.
3. Adding capability gates for pure mutable collections.

## Proposal

### P1. Naming rule

Use `Mutable*` as the only mutable collection prefix.

Canonical:

1. `MutableList`

Rejected:

1. `MutList`
2. `MList`
3. Reusing `List` with mutable semantics
4. Using `Array` to imply mutability

### P2. Placement rule

Constructor surface is module-qualified under `collections`.

Canonical:

1. `import collections`
2. `collections.MutableList.new(...)`

Properties:

1. Pure API: import required, no `with`, no manifest capability grant.
2. Type remains nameable in annotations (`MutableList<Int>`), unless a future RFC changes type-surface policy.
3. Constructor is not prelude-global.

### P3. Scope rule

This RFC standardizes `MutableList` only.

1. `MutableList` is the first and only mutable collection in scope now.
2. Additional mutable collections require separate RFCs with workload evidence.
3. `Array` is explicitly out of scope for mutability planning here.

### P4. Canonical examples

```kyokara
import collections

fn demo() -> Int {
  let xs = collections.MutableList.new().push(1).push(2)
  let ys = xs.set(0, 9)
  ys.get(0).unwrap_or(0)
}
```

```kyokara
import collections as c

fn typed() -> Int {
  let xs: MutableList<Int> = c.MutableList.new().push(3).push(4)
  xs.len()
}
```

## RFC Alignment

### RFC 0001 (API Surface Law)

This RFC strengthens surface predictability by using one explicit mutability prefix and avoiding shorthand aliases.

### RFC 0004 (Module Taxonomy)

This RFC follows module policy:

1. Specialized pure APIs are module-qualified under `collections`.
2. Mutability does not imply capability authority.

## Rollout

v0 rollout policy for this RFC:

1. Standardize naming/placement around `collections.MutableList`.
2. Keep immutable prelude collections unchanged.
3. No migration-hint diagnostics by default.

## Alternatives Considered

### A1. Use short prefix (`Mut*`)

Pros:

1. Shorter call sites.

Cons:

1. More ambiguous naming.
2. Lower AI predictability.

Decision: reject.

### A2. Put mutable collections in prelude

Pros:

1. Fewer imports.

Cons:

1. Larger global surface.
2. Weaker signal that mutable tools are specialized.

Decision: reject.

### A3. Use `Array` as mutable default

Pros:

1. Familiar in some ecosystems.

Cons:

1. Ambiguous semantics across languages.
2. Conflicts with explicit naming goal.

Decision: reject.

## Acceptance Criteria

1. RFC text clearly standardizes `MutableList` (not `Array`) as the first mutable collection target.
2. Naming rule is explicit (`Mutable*` only).
3. Placement rule is explicit (`collections.*` constructor surface).
4. Docs reference RFC 0005 as the mutability naming/placement source of truth.

## Open Questions

1. Should mutable method return conventions be standardized (`Unit` mutation vs chain-return) in a follow-up RFC?
2. Should import alias examples be added to user docs as canonical style for long module names?

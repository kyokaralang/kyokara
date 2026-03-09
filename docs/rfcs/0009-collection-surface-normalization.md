# RFC 0009: Collection Surface Normalization and Mutability Semantics

- Status: Draft
- Owner: Language Design
- Tracking issue: TBD
- Last updated: 2026-03-06

## Summary

Define one canonical collection policy for v0 language freeze:

1. Immutable collections keep simple canonical names (`List`, `Map`, `Set`, `Deque`, `PriorityQueue`).
2. Mutable collections use explicit `Mutable*` naming (`MutableList`, `MutableMap`, `MutableSet`, `MutablePriorityQueue`).
3. Collection constructors are canonically module-qualified under `collections.*` for both immutable and mutable variants.
4. Method verbs stay aligned across immutable and mutable variants (`insert`, `remove`, `contains`, `len`, etc.); semantics are determined by receiver type.
5. Immutable update APIs stay first-class and are not removed.

This RFC closes the current inconsistency where some immutable collection constructors are global/type-local while mutable constructors are `collections.*`.

## Motivation

Current shape is harder than necessary for both humans and AI agents:

1. Constructor placement is inconsistent (`Map.new()` vs `collections.MutableMap.new()`).
2. Users must memorize exceptions instead of following one stable rule.
3. The surface does not clearly communicate the intended default (immutable) vs opt-in mutable tools.

Kyokara should optimize for one predictable lookup path and one mutability signal.

## Design Goals

1. One obvious constructor namespace for all collections.
2. One obvious naming rule for mutability.
3. No duplicate verb systems between mutable and immutable variants.
4. Keep immutable-first reasoning model while allowing explicit mutable performance tools.
5. Reduce API ambiguity for AI generation and refactoring.

## Non-Goals

1. Forcing mutable mirrors for every collection immediately.
2. Defining `PriorityQueue` algorithm details (heap policy, stability, etc.).
3. Changing effect/capability semantics (collections remain pure APIs).

## Proposal

### P1. Canonical collection naming

Immutable names are canonical base nouns:

1. `List`
2. `Map`
3. `Set`
4. `Deque`
5. `PriorityQueue`

Mutable names are canonical explicit variants:

1. `MutableList`
2. `MutableMap`
3. `MutableSet`
4. `MutablePriorityQueue`

`Immutable*` names are not introduced.

### P2. Canonical constructor placement

All collection constructors are canonically under `collections`:

1. `collections.List.new()`
2. `collections.Map.new()`
3. `collections.Set.new()`
4. `collections.Deque.new()`
5. `collections.PriorityQueue.new_min()` / `new_max()` (reserved for a future immutable mirror if one is justified)
6. `collections.MutableList.new()`
7. `collections.MutableMap.new()`
8. `collections.MutableMap.with_capacity(capacity)`
9. `collections.MutableSet.new()`
10. `collections.MutableSet.with_capacity(capacity)`
9. `collections.MutablePriorityQueue.new_min()` / `new_max()` (defined by RFC 0012)

Global/type-local constructor spellings for collections become non-canonical.

### P3. Method vocabulary policy

Method names should be mirrored where semantics are conceptually the same:

1. `insert`
2. `remove`
3. `contains`
4. `len`
5. `is_empty`
6. `values` / `keys` where applicable

The mutation model is carried by type, not by alternate verb names.

### P4. Semantic distinction by receiver type

Immutable collections:

1. Updates return a new value.
2. Prior aliases observe no mutation.

Mutable collections:

1. Updates are alias-visible by design.
2. Methods may return self for chaining, but mutation semantics are in-place.

### P5. Immutable update APIs remain required

Immutable update methods on `List`/`Map`/`Set` remain part of the canonical surface.

Reason:

1. They support pure expression-oriented code.
2. They reduce accidental aliasing side effects.
3. They are essential for deterministic AI transformations.

### P6. Mirror policy for future collections

Dual immutable/mutable variants are optional, not mandatory.

Rule:

1. Add both only when both are justified by workload and ergonomics.
2. If only one is introduced initially, naming must still follow this RFC (`PriorityQueue` or `MutablePriorityQueue`, not alternate nouns).

## Canonical Examples

```kyokara
import collections

fn immutable_example() -> Int {
  let m = collections.Map.new().insert("a", 1).insert("b", 2)
  m.len()
}
```

```kyokara
import collections

fn mutable_example() -> Int {
  let m = collections.MutableMap.new().insert("a", 1)
  let alias = m
  m.insert("a", 9)
  alias.get("a").unwrap_or(0)
}
```

## RFC Alignment and Amendments

### RFC 0001

This RFC amends constructor placement for collections:

1. Collection constructors are module-qualified under `collections.*`.
2. Constructor examples in RFC 0001 using unqualified `List.new()` / `Map.new()` / `Set.new()` should be updated.

### RFC 0004

This RFC strengthens RFC 0004 by removing the remaining placement split between immutable and mutable collections.

### RFC 0005 and RFC 0008

This RFC keeps their naming principle (`Mutable*`) and generalizes placement symmetry across immutable + mutable collection families.

### RFC 0012

RFC 0012 resolves the priority-queue rollout question for v1:

1. `MutablePriorityQueue` ships first.
2. Any immutable `PriorityQueue` mirror remains follow-up work, not v1 surface.

## Migration Policy (v0 freeze window)

Because the language surface is not frozen yet:

1. Prefer direct normalization over long deprecation windows.
2. Keep diagnostics and autofix hints focused on canonical `collections.*` constructor rewrites.

## Acceptance Criteria

1. One canonical constructor namespace for all collections is documented and enforced.
2. Mutability is encoded only by `Mutable*` type naming.
3. Immutable collection update APIs remain present.
4. Method vocabulary is aligned across mutable and immutable variants where concepts match.
5. Docs and examples use the canonical collection constructor forms.

## Open Questions

1. Should non-canonical constructor spellings be hard errors immediately or staged via warning + autofix first?
2. Should formatter auto-rewrite non-canonical collection constructor forms to `collections.*` in v0?

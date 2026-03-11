# RFC 0009: Collection Surface Freeze

- Status: Draft
- Owner: Language Design
- Tracking issue: TBD
- Last updated: 2026-03-11

## Summary

Freeze Kyokara's collection model around one rule:

1. Unprefixed collections are the ordinary immutable value types: `List`, `Map`, `Set`, `BitSet`, `Deque`.
2. `Mutable*` collections are the canonical incremental build and edit path.
3. No method name may mean "returns a new value" on an immutable type and "mutates in place" on its mutable twin.
4. Immutable collections keep queries, traversal, and only clearly value-style transforms.
5. Conversions between mutable and immutable forms are explicit and symmetric.

This RFC replaces the previous direction in this document. The older "same verbs on mutable and immutable twins" model is rejected.

## Motivation

The old surface had an avoidable ambiguity:

1. The same method name could mean copy-on-write on an immutable value and alias-visible mutation on a mutable value.
2. That ambiguity made code review, generated code, docs, and refactors harder than necessary.
3. It also made it too easy to write code that looked uniform while hiding materially different mutation semantics.

Before surface freeze, Kyokara should make the split explicit:

1. Immutable collections are snapshots and value-style transforms.
2. Mutable collections are the edit path.
3. Crossing that boundary is always spelled explicitly.

## Goals

1. One stable naming rule for immutable vs mutable collection families.
2. One stable law for where edit verbs live.
3. No mutable/immutable twin pair with same-name methods but different mutation semantics.
4. Explicit, symmetric conversions for every twin pair.
5. A surface that is easy to teach, generate, lint, and mechanically rewrite.

## Non-Goals

1. Adding `Immutable*` aliases.
2. Adding immutable `PriorityQueue` in this cleanup.
3. Preserving source compatibility with the pre-freeze collection surface.
4. Adding constructor shortcuts such as `List.of`, `Map.of`, or `Set.of`.

## Frozen Model

### Type Names

Keep these immutable value types:

1. `List`
2. `Map`
3. `Set`
4. `BitSet`
5. `Deque`

Keep these mutable collection types:

1. `MutableList`
2. `MutableMap`
3. `MutableSet`
4. `MutableBitSet`
5. `MutableDeque`

Keep `MutablePriorityQueue` as mutable-only.

Do not introduce `ImmutableList`, `ImmutableMap`, `ImmutableSet`, or similar names.

### Collection Law

The language-level rule is:

1. Queries mirror across immutable and mutable twins where meaningful.
2. Traversal mirrors across immutable and mutable twins where meaningful.
3. Mutable collections own edit and mutation verbs.
4. Immutable collections own value-style transforms only.
5. No same-name mutable/immutable pair may differ only in mutation semantics.

In practice:

1. `len`, `is_empty`, `contains`, `get`, traversal producers, and similar read-only operations should line up across twins where the operation makes sense.
2. `push`, `insert`, `remove`, `set`, `update`, and similar edit verbs belong on mutable collections.
3. Immutable operations that return a changed value must use value-style names such as `sorted`, `reversed`, `with_bit`, `appended`, or `popped_front`.

### Canonical Conversions

Every immutable/mutable twin pair has explicit bidirectional conversion:

1. `collections.MutableList.from_list(xs)` and `xs.to_list()`
2. `collections.MutableMap.from_map(m)` and `m.to_map()`
3. `collections.MutableSet.from_set(s)` and `s.to_set()`
4. `collections.MutableBitSet.from_bitset(bs)` and `bs.to_bitset()`
5. `collections.MutableDeque.from_deque(q)` and `q.to_deque()`

These conversions are:

1. Explicit
2. Symmetric
3. The canonical way to move between build/edit and snapshot/value forms

`MutablePriorityQueue` remains mutable-only, so this RFC does not define an immutable twin conversion for it.

### Constructor Namespace

Collection constructors remain under `collections.*`.

This cleanup does not add `List.of`, `Map.of`, or `Set.of`.

## Family Surface

### List and MutableList

`List` keeps:

1. Queries and traversal: `len`, `get`, `head`, `tail`, `is_empty`, `contains`, traversal family, `binary_search`
2. Value transforms: `reversed()`, `concat(ys)`, `sorted()`, `sorted_by(f)`

`List` loses edit and update verbs:

1. `push`
2. `set`
3. `update`

`MutableList` keeps edit verbs:

1. `push`
2. `insert`
3. `last`
4. `pop`
5. `extend`
6. `get`
7. `set`
8. `delete_at`
9. `remove_at`
10. `update`
11. indexing

`MutableList` also keeps mirrored queries and traversal where meaningful:

1. `len`
2. `head`
3. `tail`
4. `is_empty`
5. `contains`
6. traversal family
7. `binary_search`

`MutableList` gains in-place transforms:

1. `reverse()`
2. `sort()`
3. `sort_by(f)`

### Map and MutableMap

`Map` becomes query and traversal only:

1. `get`
2. `contains`
3. `len`
4. `keys`
5. `values`
6. `is_empty`

`Map` loses persistent edit verbs:

1. `insert`
2. `remove`

`MutableMap` keeps:

1. `insert`
2. `remove`
3. `get`
4. `contains`
5. `len`
6. `keys`
7. `values`
8. `is_empty`
9. `get_or_insert_with`
10. `with_capacity`

### Set and MutableSet

`Set` becomes query and traversal only:

1. `contains`
2. `len`
3. `is_empty`
4. `values`

`Set` loses:

1. `insert`
2. `remove`

`MutableSet` keeps:

1. `insert`
2. `remove`
3. `contains`
4. `len`
5. `is_empty`
6. `values`
7. `with_capacity`

### BitSet and MutableBitSet

`BitSet` keeps queries and value operations:

1. `test`
2. `count`
3. `size`
4. `is_empty`
5. `values`
6. `union`
7. `intersection`
8. `difference`
9. `xor`

Immutable per-bit edits are renamed to value-style names:

1. `set(i)` becomes `with_bit(i)`
2. `reset(i)` becomes `without_bit(i)`
3. `flip(i)` becomes `toggled(i)`

`MutableBitSet` keeps imperative per-bit edits:

1. `set`
2. `reset`
3. `flip`

Mutable whole-set ops are renamed to explicit in-place verbs:

1. `union_with`
2. `intersection_with`
3. `difference_with`
4. `xor_with`

### Deque and MutableDeque

Add `collections.MutableDeque.new()` plus the twin conversions:

1. `collections.MutableDeque.from_deque(q)`
2. `q.to_deque()`

`MutableDeque` gets imperative queue verbs:

1. `push_front`
2. `push_back`
3. `pop_front`
4. `pop_back`
5. `len`
6. `is_empty`
7. traversal family

Persistent `Deque` uses value-style queue verbs:

1. `prepended(v)`
2. `appended(v)`
3. `popped_front()` -> `Option<{ value: T, rest: Deque<T> }>`
4. `popped_back()` -> `Option<{ value: T, rest: Deque<T> }>`

## Unchanged

1. `MutablePriorityQueue` remains mutable-only.
2. `Map`, `Set`, `BitSet`, and `Deque` remain immutable value types by default.
3. Collection constructor namespace stays under `collections.*`.

## Breaking Change Policy

This is a deliberate pre-freeze breaking cleanup:

1. No compatibility shim is required.
2. Mutable collections are the canonical construction and edit surface.
3. Immutable `Map` and `Set` are snapshot and query types after freeze.
4. Immutable `List` keeps only clearly value-like transforms.
5. RFC 0009 is replaced in place rather than superseded by a new RFC.

## Implementation Requirements

The implementation and repo surface must be updated together:

1. Builtin registrations
2. Name resolution
3. Type inference
4. Evaluation and intrinsics
5. Completions
6. CLI parity fixtures
7. API tests
8. Docs
9. Machine-facing docs

Repo sources, tests, fixtures, and docs should be mechanically rewritten to the canonical surface wherever possible.

`MutableDeque` must be a real builtin twin, not a documented exception around persistent `Deque`.

Docs should state explicitly:

1. Mutable collections are the canonical build and edit path.
2. Immutable collections are snapshots, traversal sources, and value transforms.

## Acceptance Criteria

This RFC is accepted only when the repo proves all of the following:

1. Queries mirror across every mutable/immutable twin pair where meaningful.
2. Traversal mirrors across every mutable/immutable twin pair where meaningful.
3. Twin conversions work in both directions.
4. Immutable collections reject removed edit verbs.
5. Mutable collections expose the canonical edit verbs.
6. `Deque` uses `appended`, `prepended`, and `popped_*`.
7. `MutableDeque` uses `push_*` and `pop_*`.
8. `BitSet` uses `with_bit`, `without_bit`, and `toggled`.
9. `MutableBitSet` uses `set`, `reset`, `flip`, and `*_with` whole-set ops.
10. No mutable/immutable twin pair has same-name methods with different mutation semantics.
11. Machine-facing docs match the frozen surface.

## Examples

### Mutable builder to immutable snapshot

```kyokara
import collections

fn build_names() -> List<String> {
  collections.MutableList.new()
    .push("a")
    .push("b")
    .push("c")
    .to_list()
}
```

### Immutable snapshot to mutable edit path

```kyokara
import collections

fn add_name(xs: List<String>, name: String) -> List<String> {
  collections.MutableList.from_list(xs)
    .push(name)
    .to_list()
}
```

### Persistent deque vs mutable deque

```kyokara
import collections

fn persistent_queue(q: Deque<Int>) -> Option<{ value: Int, rest: Deque<Int> }> {
  q.appended(1).popped_front()
}

fn mutable_queue(q: MutableDeque<Int>) -> MutableDeque<Int> {
  q.push_back(1).push_front(0)
}
```

### Immutable vs mutable bitset edits

```kyokara
import collections

fn persistent_bits() -> BitSet {
  collections.BitSet.new(16).with_bit(1).with_bit(3).toggled(1)
}

fn mutable_bits() -> MutableBitSet {
  collections.MutableBitSet.new(16).set(1).set(3).flip(1)
}
```

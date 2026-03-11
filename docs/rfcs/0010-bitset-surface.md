# RFC 0010: BitSet and MutableBitSet Surface

- Status: Implemented
- Owner: Language Design
- Tracking issue: #255 follow-up / RFC 0010 implementation lane
- Last updated: 2026-03-11

## Summary

Add a packed dense-bit collection family to Kyokara:

1. `BitSet`
2. `MutableBitSet`

These are the canonical tools for dense bounded-bit workloads such as precedence matrices, visited/frontier sets, and reachability rows. `MutableList<Bool>` remains valid but is not the intended representation for these cases.

## Decision

### Naming

Immutable keeps the simple canonical name:

1. `BitSet`

Mutable uses the explicit `Mutable*` form:

1. `MutableBitSet`

### Constructor placement

Constructors are canonically module-qualified under `collections`:

1. `collections.BitSet.new(size)`
2. `collections.MutableBitSet.new(size)`
3. `collections.MutableBitSet.from_bitset(bs)` and `bs.to_bitset()` for explicit twin conversion

Bare `BitSet.new()` / `MutableBitSet.new()` are non-canonical and not provided.

### Domain model

`BitSet` and `MutableBitSet` model a fixed dense domain of integer indices.

Rules:

1. valid indices are `0..size-1`
2. `size` is fixed at construction
3. negative or past-end indices are runtime errors
4. binary set algebra requires equal `size()` on both operands; mismatches are runtime errors

These are monomorphic builtin types, not generic `Set<Int>` aliases.

### Surface vocabulary

Per-bit methods:

Immutable `BitSet`:

1. `test(i)`
2. `with_bit(i)`
3. `without_bit(i)`
4. `toggled(i)`

Mutable `MutableBitSet`:

1. `test(i)`
2. `set(i)`
3. `reset(i)`
4. `flip(i)`

Whole-set algebra:

Immutable `BitSet`:

1. `union(other)`
2. `intersection(other)`
3. `difference(other)`
4. `xor(other)`

Mutable `MutableBitSet`:

1. `union_with(other)`
2. `intersection_with(other)`
3. `difference_with(other)`
4. `xor_with(other)`

Metadata/traversal:

Shared across both variants:

1. `count()` — set-bit cardinality
2. `size()` — domain width
3. `is_empty()`
4. `values()` — ascending set indices as a lazy traversal

Non-goals for v1:

1. aliases like `get`, `insert`, `remove`, `len`
2. auto-growing semantics
3. sparse integer-set semantics
4. cross-variant algebra (`BitSet` with `MutableBitSet`)
5. custom operators for bitset algebra

## Semantics

### BitSet

`BitSet` is immutable value storage:

1. per-bit edits use `with_bit` / `without_bit` / `toggled` and return a new `BitSet`
2. whole-set algebra uses value-style `union` / `intersection` / `difference` / `xor`
3. prior aliases do not observe mutation
4. runtime uses packed word storage with COW behavior

### MutableBitSet

`MutableBitSet` is alias-visible mutable storage:

1. per-bit edits use `set` / `reset` / `flip` and mutate in place
2. whole-set algebra uses `union_with` / `intersection_with` / `difference_with` / `xor_with` and mutates in place
3. aliases observe the mutation
4. mutating methods return the receiver for chaining
5. `values()` snapshots the current packed storage for traversal stability

## Runtime representation

Implementation requirement:

1. packed machine-word storage, not `List<Bool>`
2. immutable variant uses shared packed storage with COW updates
3. mutable variant uses alias-visible packed storage with snapshot-friendly traversal
4. `values()` iterates set bits lazily in ascending order without materializing `List<Int>`
5. whole-set algebra executes wordwise

## Rationale

This surface exists because dense bounded-bit domains keep recurring in algorithmic code and AI-generated fixes:

1. precedence constraints
2. visited/frontier tracking
3. dense reachability rows
4. compact boolean tables keyed by small integer IDs

A `MutableList<Bool>` can express the same information but not with the same cost model. In Kyokara today, booleans stored in lists are generic runtime values, not packed bits, so the density and bulk-operation gap is substantial.

## Example

```kyokara
import collections

fn main() -> Bool {
  let a = collections.BitSet.new(16).with_bit(1).with_bit(3)
  let b = collections.BitSet.new(16).with_bit(3).with_bit(4)
  let c = a.union(b)
  c.count() == 4 && c.test(4)
}
```

```kyokara
import collections

fn mark_seen(xs: List<Int>) -> MutableBitSet {
  xs.fold(collections.MutableBitSet.new(256), fn(acc: MutableBitSet, x: Int) => acc.set(x))
}
```

## Acceptance criteria

1. `BitSet` and `MutableBitSet` are globally nameable builtin types
2. constructors are available only under `collections.*`
3. explicit `from_bitset` / `to_bitset` twin conversion is available
4. method surface matches this RFC exactly
5. packed runtime representation is used
6. `values()` yields ascending indices lazily
7. wrong index type and wrong receiver/rhs type are rejected by normal type checking
8. dense-bit perf harness coverage exists in the repo-owned benchmark corpus

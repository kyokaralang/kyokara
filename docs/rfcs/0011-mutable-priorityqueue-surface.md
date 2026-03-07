# RFC 0011: MutablePriorityQueue Surface

- Status: Draft
- Owner: Language Design
- Tracking issue: [#257](https://github.com/kyokaralang/kyokara/issues/257)
- Depends on: RFC 0010
- Last updated: 2026-03-07

## Summary

Define the first shipped priority-queue surface for Kyokara as a mutable collection under `collections`.

This RFC locks:

1. mutable-only v1 scope,
2. queue-first API rather than a raw heap API,
3. explicit min/max construction,
4. `Ord`-bounded generic priority types,
5. deterministic tie behavior.

## Motivation

Weighted frontier workloads such as Dijkstra, A*, and best-first search need efficient prioritized extraction without manual linear scans.

The language already has persistent collections and specialized mutable collections. The first priority-queue feature should match that model:

1. explicit constructor placement under `collections`,
2. explicit `Mutable*` naming for alias-visible mutation,
3. predictable deterministic behavior,
4. no hidden min/max default.

## Design goals

1. Make pathfinding-style worklists direct to express.
2. Keep the surface small and explicit.
3. Reuse the `Mutable*` naming law already established for other alias-visible collections.
4. Avoid dragging raw heap or comparator policy into the first shipped surface.
5. Keep the implementation free to use a single heap engine under the hood.

## Non-goals

1. Shipping an immutable `PriorityQueue` in v1.
2. Shipping a raw `Heap`, `MinHeap`, or `MaxHeap` user-facing API in v1.
3. Constructor-time comparator closures.
4. Decrease-key, update-priority, merge, or indexed-heap operations.
5. Traversal APIs beyond what is required for queue inspection and mutation.

## Proposal

### P1. Type and placement

Add one new builtin mutable collection type:

1. `MutablePriorityQueue<P, T>` where `P: Ord`

Canonical constructor placement:

```kyokara
import collections

let pq = collections.MutablePriorityQueue.new_min<Int, String>()
```

No global constructor aliases.

### P2. Constructor surface

The v1 constructor family is:

1. `collections.MutablePriorityQueue.new_min<P: Ord, T>()`
2. `collections.MutablePriorityQueue.new_max<P: Ord, T>()`

Rules:

1. Direction is explicit at construction time.
2. There is no bare `new()` with an implicit default.
3. The implementation may share one internal heap engine for both directions.

### P3. Canonical method surface

The v1 surface is:

1. `pq.push(priority, value) -> MutablePriorityQueue<P, T>`
2. `pq.peek() -> Option<{ priority: P, value: T }>`
3. `pq.pop() -> Option<{ priority: P, value: T }>`
4. `pq.len() -> Int`
5. `pq.is_empty() -> Bool`

Method semantics:

1. `push` mutates alias-visible state and returns self for chaining, matching other `Mutable*` collections.
2. `peek` is non-destructive.
3. `pop` removes the selected element and returns only the removed pair, not a rest-structure payload.

### P4. Ordering and determinism

Selection rule:

1. `new_min()` returns the smallest priority first.
2. `new_max()` returns the largest priority first.

Tie-breaking rule:

1. Equal priorities are resolved by insertion order.
2. Earlier inserted items are returned first among equal-priority entries.
3. This rule is part of the semantic contract, not an implementation accident.

Reason:

1. deterministic execution is better for humans, tests, replay, and AI agents.

### P5. Type constraints

Priority type requirements:

1. `P` must satisfy `Ord` from RFC 0010.
2. Value type `T` is unconstrained.
3. The first implementation phase may use the subset of `Ord`-conforming builtin types and later automatically widens as RFC 0010 implementations land.

### P6. Mutation model

`MutablePriorityQueue` follows the existing mutable-collection rule:

1. updates are alias-visible,
2. constructors and methods remain pure APIs with no capability gate,
3. mutability is signaled only by the `Mutable*` type name.

### P7. Explicit out-of-scope surface

The following are intentionally not in v1:

1. `PriorityQueue<P, T>` immutable mirror type,
2. `Heap`, `MinHeap`, `MaxHeap`, `MutableMinHeap`, `MutableMaxHeap`,
3. `decrease_key`, `update_priority`, `merge`, `drain`, `iter`, or bulk-build helpers,
4. custom comparator closures,
5. traversal combinators.

## Canonical examples

```kyokara
import collections

fn frontier_demo() -> String {
  let pq = collections.MutablePriorityQueue.new_min<Int, String>()
    .push(5, "far")
    .push(1, "near")
    .push(1, "nearer")

  match (pq.pop()) {
    Some(item) => item.value,
    None => "none",
  }
}
```

Expected behavior:

1. first `pop()` returns priority `1`, value `"near"`,
2. second `pop()` returns priority `1`, value `"nearer"`,
3. insertion order is preserved within equal-priority ties.

## RFC alignment

### RFC 0004

This RFC uses the specialized-collection placement rule:

1. constructor surface is under `collections.*`.

### RFC 0009

This RFC resolves RFC 0009's priority-queue scope question:

1. priority queue ships mutable-only first,
2. `MutablePriorityQueue` is the first canonical shipped type,
3. any future immutable mirror remains follow-up work.

### RFC 0010

This RFC depends on RFC 0010 for `Ord`-bounded priority typing and should not be implemented before RFC 0010 is accepted.

## Acceptance criteria

1. One canonical mutable priority-queue surface is documented.
2. Min/max direction is explicit and never implicit.
3. Tie behavior is deterministic and insertion-stable.
4. The v1 scope is explicitly mutable-only.
5. Dijkstra/A*-style workloads are expressible without linear frontier scans once the feature is implemented.

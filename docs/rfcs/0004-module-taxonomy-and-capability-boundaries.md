# RFC 0004: Module Taxonomy and Capability Boundaries

- Status: Draft
- Owner: Language Design
- Tracking issue: TBD
- Last updated: 2026-03-04

## Summary

Define one consistent mental model for API placement and authority checks:

1. Keep only ubiquitous pure value types in prelude.
2. Place specialized pure APIs (including specialized collections) in pure modules.
3. Keep side effects in capability-scoped modules with explicit authority (`with ...`) and runtime manifest checks.

This RFC proposes moving specialized collections such as `Deque` and `PriorityQueue` under a `collections` module namespace while preserving value-owned method APIs.

## Motivation

As the stdlib grows, global type constructors increase namespace noise and reduce AI pass@1 predictability.

Current friction:

1. Specialized types (`Deque` today, `PriorityQueue` soon) add to the global surface.
2. The relationship between module import and capability authority is easy to misread.
3. Users need one simple rule for "where does this API live" and "what permission do I need".

## Design Goals

1. Small default surface: only common primitives in prelude.
2. Predictable placement: specialized APIs are discoverable in domain modules.
3. Explicit authority: effectful operations require capability declarations, not just imports.
4. Keep existing API surface law direction (RFC 0001): owner-first methods, module-qualified no-owner APIs.

## Non-Goals

1. Redesigning effect typing itself.
2. Adding new collection algorithms in this RFC.
3. Defining all future modules in detail.

## Proposal

### P1. Two-axis model: Visibility vs Authority

Every call site follows this matrix:

| Surface category | Import required | `with` required | Manifest grant required |
|---|---|---|---|
| Prelude pure value APIs (`List`, `Map`, `Set`, `String`, etc.) | No | No | No |
| Pure module APIs (`collections.*`, `math.*`, etc.) | Yes | No | No |
| Effect module APIs (`io.*`, `fs.*`, `net.*`, etc.) | Yes | Yes | Yes |

Key rule:

1. `import` controls visibility.
2. `with` + manifest control authority.
3. Import never grants authority.

### P2. Namespace tiers

#### Tier A: Prelude core (minimal)

Keep only ubiquitous pure types globally visible:

1. `Int`, `Float`, `Bool`, `String`, `Char`, `Unit`
2. `Option`, `Result`
3. `List`, `Map`, `Set`

#### Tier B: Pure feature modules

Specialized pure APIs live in modules and are imported explicitly.

Initial policy:

1. `Deque` should be exposed via `collections` (not prelude-global).
2. `PriorityQueue` should be introduced in `collections` directly.
3. Future specialized structures (for example `BitSet`) should follow the same rule.

#### Tier C: Effect modules

APIs that can perform side effects remain module-qualified and capability-scoped:

1. `io.*` with `IO`
2. `fs.*` with `FS`
3. future effect modules follow the same import + authority model

### P3. Collections placement contract

Canonical placement for specialized collection constructors:

1. `collections.Deque.new()`
2. `collections.PriorityQueue.new_min()` (or final canonical constructor naming once fixed)

Once a value exists, behavior remains owner methods:

1. `q.push_back(x)`
2. `q.pop_front()`
3. `pq.push(p, v)`
4. `pq.pop()`

### P4. Canonical examples

Pure specialized collection use:

```kyokara
import collections

fn queue_size() -> Int {
  let q = collections.Deque.new().push_back(1).push_back(2)
  q.len()
}
```

Effectful module use:

```kyokara
import io

fn main() -> Unit
with IO
{
  io.println("ok")
}
```

Invalid (visible but unauthorized):

```kyokara
import io

fn main() -> Unit {
  io.println("no") // compile error: missing with IO
}
```

## RFC 0001 alignment

This RFC clarifies RFC 0001 without changing its core laws:

1. L2/L5 remain: owner methods + type/module namespace placement.
2. L4 remains: effects are capability-scoped.
3. Add practical policy: prelude is reserved for ubiquitous pure primitives; specialized pure APIs prefer modules.

Proposed additive text in RFC 0001 (new clause, draft):

### L5.2 Prelude budget for AI predictability (`SHOULD`)

Only ubiquitous primitives should be prelude-global. Specialized pure APIs should be module-qualified to keep global surface small and predictable.

## Rollout (v0 hard-break policy)

1. Move `Deque` constructor surface to `collections` namespace.
2. Keep method behavior unchanged.
3. Introduce `PriorityQueue` directly under `collections`.
4. Update docs/examples/completions to module-first specialized collection references.

No migration-hint policy is required in v0 unless explicitly chosen.

## Alternatives considered

### A1. Keep all collections in prelude

Pros:

1. Shortest call-site spelling.

Cons:

1. Grows global surface continuously.
2. Reduces discoverability predictability for AI generation.

Decision: reject.

### A2. Capability-gate pure collection modules

Pros:

1. Single mechanism for modules.

Cons:

1. Conceptually wrong: pure APIs should not require authority.
2. Blurs visibility vs authority model.

Decision: reject.

### A3. Keep Deque global, put only future types in modules

Pros:

1. Smaller immediate break.

Cons:

1. Inconsistent placement rules.
2. Leaves avoidable legacy exception in the mental model.

Decision: reject.

## Acceptance Criteria

1. The import/authority matrix is documented and consistent across docs.
2. Specialized collections are module-namespaced, not prelude-global.
3. Effect modules require both import and capability authority.
4. Examples and diagnostics reflect one canonical model.

## Open Questions

1. Should module-qualified type constructors require fully qualified usage always, or allow imported aliases with canonical formatter output?
2. Should capability names be normalized to match module names (`IO`/`io` mapping policy) in diagnostics and docs?
